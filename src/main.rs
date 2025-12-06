use anyhow::{Context, Result};
use clap::Parser;
use daemonize::Daemonize;
use matrix_bot_ical::config::{self, Config, ReminderType, should_ignore_user};
use matrix_bot_ical::ical::IcalCalendar;
use matrix_sdk::{
    Client, Room, RoomState, SessionMeta, SessionTokens,
    authentication::matrix::MatrixSession,
    config::SyncSettings,
    ruma::events::room::member::{MembershipState, StrippedRoomMemberEvent},
    ruma::events::room::message::{
        MessageType, OriginalSyncRoomMessageEvent, RoomMessageEventContent,
    },
    ruma::{RoomId, UserId, device_id},
};
use std::fs::{self, OpenOptions};
use tokio_cron_scheduler::{Job, JobScheduler};

#[derive(Parser)]
#[command(name = "matrix-bot-ical")]
#[command(about = "A Matrix bot for iCal / WebCal")]
struct Cli {
    /// Config file path
    #[arg(short, long, default_value = "bot.toml")]
    config: String,

    /// Daemonize the process
    #[arg(short = 'd', long, default_value = "false")]
    daemonize: bool,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    println!("Using config file: {}", cli.config);
    println!("Daemonize: {}", cli.daemonize);

    // Read and parse config file
    let config_content = fs::read_to_string(&cli.config)
        .with_context(|| format!("Failed to read config file '{}'", cli.config))?;

    // Parse configuration from TOML
    let config = Config::from_toml(&config_content).context("Failed to parse config")?;

    println!("Config loaded:");
    config.print();

    // Validate reminder configurations before starting bot
    validate_reminders(&config)?;

    if (IcalCalendar::from_url_blocking(&config.webcal)).is_ok() {
        println!("Calendar fetched and parsed: {}", &config.webcal);
    }

    // Daemonize if requested
    if cli.daemonize {
        let log_file_handle = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&config.log_file)
            .with_context(|| format!("Failed to open log file '{}'", config.log_file))?;

        let daemonize = Daemonize::new()
            .pid_file("/tmp/matrix-bot-ical.pid")
            .working_directory(&config.working_dir)
            .stdout(
                log_file_handle
                    .try_clone()
                    .context("Failed to clone log file handle")?,
            )
            .stderr(log_file_handle);

        daemonize.start().context("Failed to daemonize")?;

        println!("Successfully daemonized, PID: {}", std::process::id());
        config.print();
    }

    run_bot(&config)?;
    println!("Bye.");
    Ok(())
}

#[tokio::main]
async fn run_bot(config: &Config) -> Result<()> {
    println!("Starting Matrix bot with homeserver: {}", config.homeserver);

    // Create client
    let client = Client::builder()
        .homeserver_url(&config.homeserver)
        .build()
        .await?;

    // Create a MatrixSession with existing access token
    let user_id = UserId::parse(&config.username)
        .map_err(|e| anyhow::anyhow!("Invalid user ID '{}': {}", config.username, e))?;

    let session = MatrixSession {
        meta: SessionMeta {
            user_id,
            device_id: device_id!("matrix-bot-ical").to_owned(),
        },
        tokens: SessionTokens {
            access_token: config.access_token.clone(),
            refresh_token: None,
        },
    };

    // Restore the session with access token
    client
        .matrix_auth()
        .restore_session(session, matrix_sdk::store::RoomLoadSettings::default())
        .await?;

    println!("Successfully logged in as {}", config.username);

    // Initial sync to avoid responding to old messages
    let response = client.sync_once(SyncSettings::default()).await?;
    println!("Initial sync completed");

    // Get bot user ID for filtering
    let bot_user_id = client
        .user_id()
        .expect("Client should have a user ID")
        .to_owned();

    // Add event handler for room messages
    let bot_filtering = config.bot_filtering.clone();
    let config_clone = config.clone();
    client.add_event_handler(
        move |event: OriginalSyncRoomMessageEvent, room: Room| async move {
            on_room_message(event, room, &bot_user_id, &bot_filtering, &config_clone).await
        },
    );

    // Add event handler for autojoining rooms when invited
    client.add_event_handler(on_stripped_state_member);

    // Setup cron scheduler for reminders
    setup_reminder_scheduler(&client, config).await?;

    // Start continuous sync
    let settings = SyncSettings::default().token(response.next_batch);
    println!("Starting continuous sync...");
    client.sync(settings).await?;

    Ok(())
}

async fn on_room_message(
    event: OriginalSyncRoomMessageEvent,
    room: Room,
    bot_user_id: &UserId,
    bot_filtering: &config::BotFilteringConfig,
    config: &Config,
) {
    // Only respond to messages in joined rooms
    if room.state() != RoomState::Joined {
        return;
    }

    let MessageType::Text(text_content) = event.content.msgtype else {
        return;
    };

    // Check if sender should be ignored based on bot filtering configuration
    if should_ignore_user(event.sender.as_str(), bot_user_id.as_str(), bot_filtering) {
        println!("Ignoring message from filtered user: {}", event.sender);
        return;
    }

    // Check if message is for meetings/events
    if text_content.body.starts_with("!meetings") || text_content.body.starts_with("!events") {
        println!(
            "Received meetings/events request in room {}",
            room.room_id()
        );

        let response =
            RoomMessageEventContent::text_markdown(handle_meetings_events_request(config).await);

        if let Err(e) = room.send(response).await {
            eprintln!("Failed to send meetings/events message: {}", e);
        }
    }
    // Check if message is for meeting/event
    else if text_content.body.starts_with("!meeting") || text_content.body.starts_with("!event") {
        println!("Received meeting/event request in room {}", room.room_id());

        let response =
            RoomMessageEventContent::text_markdown(handle_meeting_event_request(config).await);

        if let Err(e) = room.send(response).await {
            eprintln!("Failed to send meeting/event message: {}", e);
        }
    }
}

async fn on_stripped_state_member(event: StrippedRoomMemberEvent, client: Client, room: Room) {
    // Only process invitations for the bot itself
    if event.state_key != client.user_id().expect("Client should have a user ID") {
        return;
    }

    // Check if this is an invitation
    if event.content.membership == MembershipState::Invite {
        println!("Received invitation to room {}", room.room_id());

        // Join the room with retry logic
        let room_id = room.room_id().to_owned();
        tokio::spawn(async move {
            let mut delay = 2;

            while let Err(e) = room.join().await {
                eprintln!(
                    "Failed to join room {} ({}), retrying in {}s",
                    room_id, e, delay
                );
                tokio::time::sleep(tokio::time::Duration::from_secs(delay)).await;
                delay *= 2;

                if delay > 3600 {
                    eprintln!("Can't join room {} after multiple retries", room_id);
                    break;
                }
            }

            if (room.join().await).is_ok() {
                println!("Successfully joined room {}", room_id);
            }
        });
    }
}

fn format_ical_date(ical_date: &str) -> String {
    match chrono::DateTime::parse_from_str(ical_date, "%Y%m%dT%H%M%SZ") {
        Ok(dt) => dt.format("%a, %b %d, %Y at %I:%M %p").to_string(),
        Err(_) => {
            // Try parsing without timezone
            match chrono::NaiveDateTime::parse_from_str(ical_date, "%Y%m%dT%H%M%S") {
                Ok(dt) => dt.format("%a, %b %d, %Y at %I:%M %p").to_string(),
                Err(_) => ical_date.to_string(), // Return original if parsing fails
            }
        }
    }
}

async fn handle_meeting_event_request(config: &Config) -> String {
    if config.webcal.is_empty() {
        return "No webcal URL configured".to_string();
    }

    let calendar = match IcalCalendar::from_url(&config.webcal).await {
        Ok(calendar) => calendar,
        Err(_) => return "There was a problem fetching the calendar".to_string(),
    };

    let current_time = chrono::Utc::now().format("%Y%m%dT%H%M%SZ").to_string();
    let upcoming_events = calendar.get_upcoming_events_limited(&current_time, Some(1));

    if upcoming_events.is_empty() {
        return "No upcoming events found.".to_string();
    }

    let event = upcoming_events[0];
    let mut response = String::new();
    response.push_str("# Next Meeting/Event\n\n");

    if let Some(summary) = &event.summary {
        if let Some(url) = &event.url {
            response.push_str(&format!("**[{}]({})**\n", summary, url));
        } else {
            response.push_str(&format!("**{}**\n", summary));
        }

        if let Some(start_time) = &event.start_time {
            response.push_str(&format!("* Starts: {}\n", format_ical_date(start_time)));
        }

        if let Some(end_time) = &event.end_time {
            response.push_str(&format!("* Ends: {}\n", format_ical_date(end_time)));
        }

        if let Some(location) = &event.location {
            response.push_str(&format!("* Location: {}\n", location));
        }

        response.push_str("\n\n");
    }

    // Add info URL if configured
    if let Some(info_url) = &config.info_url {
        response.push_str(&format!("\nFor more information: {}\n", info_url));
    }

    response
}

async fn handle_meetings_events_request(config: &Config) -> String {
    if config.webcal.is_empty() {
        return "No webcal URL configured".to_string();
    }

    let calendar = match IcalCalendar::from_url(&config.webcal).await {
        Ok(calendar) => calendar,
        Err(_) => return "There was a problem fetching the calendar".to_string(),
    };

    let current_time = chrono::Utc::now().format("%Y%m%dT%H%M%SZ").to_string();
    let upcoming_events = calendar.get_upcoming_events(&current_time);

    if upcoming_events.is_empty() {
        return "No upcoming events found.".to_string();
    }

    let mut response = String::new();
    response.push_str("# Upcoming Meetings/Events\n\n");

    for event in upcoming_events {
        if let Some(summary) = &event.summary {
            if let Some(url) = &event.url {
                response.push_str(&format!("**[{}]({})**\n", summary, url));
            } else {
                response.push_str(&format!("**{}**\n", summary));
            }

            if let Some(start_time) = &event.start_time {
                response.push_str(&format!("* Starts: {}\n", format_ical_date(start_time)));
            }

            if let Some(end_time) = &event.end_time {
                response.push_str(&format!("* Ends: {}\n", format_ical_date(end_time)));
            }

            if let Some(location) = &event.location {
                response.push_str(&format!("* Location: {}\n", location));
            }

            response.push_str("\n\n");
        }
    }

    // Add info URL if configured
    if let Some(info_url) = &config.info_url {
        response.push_str(&format!("\nFor more information: {}\n", info_url));
    }

    response
}

fn validate_reminders(config: &Config) -> Result<()> {
    for (i, reminder) in config.reminders.iter().enumerate() {
        // Validate cron expression
        if let Err(e) = Job::new_async(&reminder.cron, move |_uuid, _l| Box::pin(async {})) {
            return Err(anyhow::anyhow!(
                "Invalid cron expression in reminder #{}: '{}'. Error: {}",
                i + 1,
                reminder.cron,
                e
            ));
        }

        // Validate room ID
        if let Err(e) = RoomId::parse(&reminder.matrix_room) {
            return Err(anyhow::anyhow!(
                "Invalid Matrix room ID in reminder #{}: '{}'. Error: {}",
                i + 1,
                reminder.matrix_room,
                e
            ));
        }
    }

    if config.reminders.is_empty() {
        println!("No reminders configured");
    } else {
        println!("Validated {} reminder(s)", config.reminders.len());
    }

    Ok(())
}

async fn setup_reminder_scheduler(client: &Client, config: &Config) -> Result<()> {
    let scheduler = JobScheduler::new().await?;

    for (i, reminder) in config.reminders.iter().enumerate() {
        let client_clone = client.clone();
        let config_clone = config.clone();
        let reminder_type = reminder.reminder_type.clone();
        let room_id = reminder.matrix_room.clone();

        let job = Job::new_async(&reminder.cron, move |_uuid, _l| {
            let client_clone = client_clone.clone();
            let config_clone = config_clone.clone();
            let room_id = room_id.clone();
            let reminder_type = reminder_type.clone();

            Box::pin(async move {
                send_scheduled_reminder(&client_clone, &config_clone, &room_id, &reminder_type)
                    .await;
            })
        })?;

        scheduler.add(job).await?;
        println!(
            "Scheduled reminder #{}: {} -> {:?} in room {}",
            i + 1,
            reminder.cron,
            reminder.reminder_type,
            reminder.matrix_room
        );
    }

    if !config.reminders.is_empty() {
        scheduler.start().await?;
        println!(
            "Reminder scheduler started with {} jobs",
            config.reminders.len()
        );
    }

    Ok(())
}

async fn send_scheduled_reminder(
    client: &Client,
    config: &Config,
    room_id: &str,
    reminder_type: &ReminderType,
) {
    let room_id = match RoomId::parse(room_id) {
        Ok(id) => id,
        Err(e) => {
            eprintln!("Invalid room ID '{}': {}", room_id, e);
            return;
        }
    };

    let room = match client.get_room(&room_id) {
        Some(room) => room,
        None => {
            eprintln!("Bot is not in room '{}'", room_id);
            return;
        }
    };

    let message = match reminder_type {
        ReminderType::NextMeeting => handle_meeting_event_request(config).await,
        ReminderType::AllUpcomingMeetings => handle_meetings_events_request(config).await,
    };

    let response = RoomMessageEventContent::text_markdown(message);

    if let Err(e) = room.send(response).await {
        eprintln!(
            "Failed to send scheduled reminder to room '{}': {}",
            room_id, e
        );
    } else {
        println!("Sent scheduled reminder to room '{}'", room_id);
    }
}
