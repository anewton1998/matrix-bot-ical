use anyhow::{Context, Result};
use clap::Parser;
use daemonize::Daemonize;
use matrix_bot_ical::config::{self, Config, should_ignore_user};
use matrix_bot_ical::ical::IcalCalendar;
use matrix_sdk::{
    Client, Room, RoomState, SessionMeta, SessionTokens,
    authentication::matrix::MatrixSession,
    config::SyncSettings,
    ruma::events::room::member::{MembershipState, StrippedRoomMemberEvent},
    ruma::events::room::message::{
        MessageType, OriginalSyncRoomMessageEvent, RoomMessageEventContent,
    },
    ruma::{UserId, device_id},
};
use std::fs::{self, OpenOptions};

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

#[tokio::main]
async fn main() -> Result<()> {
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

        // Bot logic runs here after daemonizing
        run_bot(&config).await?;
    } else {
        // Non-daemon bot logic
        run_bot(&config).await?;
    }

    Ok(())
}

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

    // Check if message is for meeting/event
    if text_content.body.starts_with("!meeting") || text_content.body.starts_with("!event") {
        println!("Received meeting/event request in room {}", room.room_id());

        let response =
            RoomMessageEventContent::text_markdown(handle_meeting_event_request(config).await);

        if let Err(e) = room.send(response).await {
            eprintln!("Failed to send meeting/event message: {}", e);
        }
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
    response.push_str("# Next Meeting\n\n");

    if let Some(summary) = &event.summary {
        response.push_str(&format!("**{}**", summary));

        if let Some(start_time) = &event.start_time {
            response.push_str(&format!(" - Starts: {}", start_time));
        }

        if let Some(location) = &event.location {
            response.push_str(&format!(" - Location: {}", location));
        }

        if let Some(description) = &event.description {
            response.push_str(&format!("\n{}", description));
        }

        response.push_str("\n\n");
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
    response.push_str("# Upcoming Events\n\n");

    for event in upcoming_events {
        if let Some(summary) = &event.summary {
            response.push_str(&format!("**{}**", summary));

            if let Some(start_time) = &event.start_time {
                response.push_str(&format!(" - Starts: {}", start_time));
            }

            if let Some(location) = &event.location {
                response.push_str(&format!(" - Location: {}", location));
            }

            if let Some(description) = &event.description {
                response.push_str(&format!("\n{}", description));
            }

            response.push_str("\n\n");
        }
    }

    response
}
