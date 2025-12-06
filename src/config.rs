use anyhow::{Result, anyhow};
use toml::Value;

/// Configuration for bot message filtering.
#[derive(Debug, Clone)]
pub struct BotFilteringConfig {
    /// Whether to ignore messages from bot itself
    pub ignore_self: bool,
    /// Whether to ignore messages from users with "bot" in their username
    pub ignore_bots: bool,
    /// Specific list of user IDs to ignore
    pub ignored_users: Vec<String>,
}

/// Reminder type for scheduled notifications.
#[derive(Debug, Clone, PartialEq)]
pub enum ReminderType {
    NextMeeting,
    AllUpcomingMeetings,
}

/// Configuration for a scheduled reminder.
#[derive(Debug, Clone)]
pub struct ReminderConfig {
    /// Cron expression for when to send reminder
    pub cron: String,
    /// Type of reminder to send
    pub reminder_type: ReminderType,
    /// Matrix room ID where to send the reminder
    pub matrix_room: String,
}

impl Default for BotFilteringConfig {
    fn default() -> Self {
        Self {
            ignore_self: true,
            ignore_bots: false,
            ignored_users: Vec::new(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Config {
    pub homeserver: String,
    pub username: String,
    pub access_token: String,
    pub log_file: String,
    pub working_dir: String,
    pub webcal: String,
    pub info_url: Option<String>,
    pub reminders: Vec<ReminderConfig>,
    pub bot_filtering: BotFilteringConfig,
}

impl Config {
    pub fn from_toml(toml_str: &str) -> Result<Self> {
        let config: Value =
            toml::from_str(toml_str).map_err(|e| anyhow!("Failed to parse TOML: {}", e))?;

        Ok(Config {
            homeserver: config
                .get("homeserver")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow!("Missing 'homeserver' in config file"))?
                .to_string(),
            username: config
                .get("username")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow!("Missing 'username' in config file"))?
                .to_string(),
            access_token: config
                .get("access_token")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow!("Missing 'access_token' in config file"))?
                .to_string(),
            log_file: config
                .get("log_file")
                .and_then(|v| v.as_str())
                .unwrap_or("bot.log")
                .to_string(),
            working_dir: config
                .get("working_directory")
                .and_then(|v| v.as_str())
                .unwrap_or(".")
                .to_string(),
            webcal: config
                .get("webcal")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            info_url: config
                .get("info_url")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
            reminders: parse_reminders_config(&config)?,
            bot_filtering: parse_bot_filtering_config(&config)?,
        })
    }

    pub fn print(&self) {
        println!("Configuration:");
        println!("  Homeserver: {}", self.homeserver);
        println!("  Username: {}", self.username);
        println!(
            "  Access Token: {}",
            if self.access_token.is_empty() {
                "[empty]"
            } else {
                "[set]"
            }
        );
        println!("  Log File: {}", self.log_file);
        println!("  Working Directory: {}", self.working_dir);
        println!("  Webcal: {}", self.webcal);
        match &self.info_url {
            Some(url) => println!("  Info URL: {}", url),
            None => println!("  Info URL: [not set]"),
        }
        println!("  Reminders:");
        if self.reminders.is_empty() {
            println!("    [none]");
        } else {
            for (i, reminder) in self.reminders.iter().enumerate() {
                println!("    {}: {} -> {:?} in room {}", i + 1, reminder.cron, reminder.reminder_type, reminder.matrix_room);
            }
        }
        println!("  Bot Filtering:");
        println!("    Ignore Self: {}", self.bot_filtering.ignore_self);
        println!("    Ignore Bots: {}", self.bot_filtering.ignore_bots);
        if !self.bot_filtering.ignored_users.is_empty() {
            println!("    Ignored Users:");
            for user in &self.bot_filtering.ignored_users {
                println!("      {}", user);
            }
        } else {
            println!("    Ignored Users: [none]");
        }
    }
}

/// Parse reminders configuration from TOML value.
fn parse_reminders_config(config: &Value) -> Result<Vec<ReminderConfig>> {
    let reminders_config = config.get("reminders");
    
    if let Some(reminders_array) = reminders_config.and_then(|v| v.as_array()) {
        let mut reminders = Vec::new();
        
        for reminder_value in reminders_array {
            if let Some(reminder_table) = reminder_value.as_table() {
                let cron = reminder_table
                    .get("cron")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| anyhow!("Missing 'cron' in reminder configuration"))?
                    .to_string();
                
                let reminder_type_str = reminder_table
                    .get("reminder_type")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| anyhow!("Missing 'reminder_type' in reminder configuration"))?;
                
                let reminder_type = match reminder_type_str {
                    "NextMeeting" => ReminderType::NextMeeting,
                    "AllUpcomingMeetings" => ReminderType::AllUpcomingMeetings,
                    _ => return Err(anyhow!("Invalid reminder_type: {}", reminder_type_str)),
                };
                
                let matrix_room = reminder_table
                    .get("matrix_room")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| anyhow!("Missing 'matrix_room' in reminder configuration"))?
                    .to_string();
                
                reminders.push(ReminderConfig {
                    cron,
                    reminder_type,
                    matrix_room,
                });
            }
        }
        
        Ok(reminders)
    } else {
        // No reminders section, return empty vector
        Ok(Vec::new())
    }
}

/// Parse bot filtering configuration from TOML value.
fn parse_bot_filtering_config(config: &Value) -> Result<BotFilteringConfig> {
    let bot_filtering_config = config.get("bot_filtering");

    if let Some(bot_config) = bot_filtering_config {
        // Parse ignore_self
        let ignore_self = bot_config
            .get("ignore_self")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);

        // Parse ignore_bots
        let ignore_bots = bot_config
            .get("ignore_bots")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        // Parse ignored_users
        let ignored_users = bot_config
            .get("ignored_users")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str())
                    .map(|s| s.to_string())
                    .collect()
            })
            .unwrap_or_default();

        Ok(BotFilteringConfig {
            ignore_self,
            ignore_bots,
            ignored_users,
        })
    } else {
        // No bot_filtering section, use defaults
        Ok(BotFilteringConfig::default())
    }
}

/// Check if a user ID should be ignored based on bot filtering configuration.
pub fn should_ignore_user(user_id: &str, bot_user_id: &str, config: &BotFilteringConfig) -> bool {
    // Check if it's bot itself
    if config.ignore_self && user_id == bot_user_id {
        return true;
    }

    // Check if user is in ignored list
    if config.ignored_users.contains(&user_id.to_string()) {
        return true;
    }

    // Check if user has "bot" in their username (case-insensitive)
    if config.ignore_bots && user_id.to_lowercase().contains("bot") {
        return true;
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use indoc::indoc;

    #[test]
    fn test_minimal_config_parsing() {
        // Given a minimal TOML configuration with only required fields
        let toml_str = indoc! {"
            homeserver = \"https://matrix.example.com\"
            username = \"@bot:example.com\"
            access_token = \"secret_token\"
        "};

        // When parsing the TOML configuration
        let config = Config::from_toml(toml_str).unwrap();

        // Then all required fields should be parsed correctly and defaults should be applied
        assert_eq!(config.homeserver, "https://matrix.example.com");
        assert_eq!(config.username, "@bot:example.com");
        assert_eq!(config.access_token, "secret_token");
        assert_eq!(config.log_file, "bot.log");
        assert_eq!(config.working_dir, ".");
        assert_eq!(config.webcal, "");
        assert_eq!(config.info_url, None);
        assert!(config.reminders.is_empty());
        // Bot filtering should use defaults when not specified
        assert!(config.bot_filtering.ignore_self);
        assert!(!config.bot_filtering.ignore_bots);
        assert!(config.bot_filtering.ignored_users.is_empty());
    }

    #[test]
    fn test_full_config_parsing() {
        // Given a complete TOML configuration with all optional fields
        let toml_str = indoc! {"
            homeserver = \"https://matrix.example.com\"
            username = \"@bot:example.com\"
            access_token = \"secret_token\"
            log_file = \"/var/log/bot.log\"
            working_directory = \"/app\"
            webcal = \"https://example.com/calendar.ics\"
            info_url = \"https://example.com/info\"

            [[reminders]]
            cron = \"0 9 * * 1-5\"
            reminder_type = \"NextMeeting\"
            matrix_room = \"!roomid:example.com\"

            [[reminders]]
            cron = \"0 8 * * 1\"
            reminder_type = \"AllUpcomingMeetings\"
            matrix_room = \"!roomid:example.com\"

            [bot_filtering]
            ignore_self = false
            ignore_bots = true
            ignored_users = [\"@spam-bot:example.com\", \"@announcement-bot:example.com\"]
        "};

        // When parsing the TOML configuration
        let config = Config::from_toml(toml_str).unwrap();

        // Then all fields should be parsed with their specified values
        assert_eq!(config.homeserver, "https://matrix.example.com");
        assert_eq!(config.username, "@bot:example.com");
        assert_eq!(config.access_token, "secret_token");
        assert_eq!(config.log_file, "/var/log/bot.log");
assert_eq!(config.working_dir, "/app");
        assert_eq!(config.webcal, "https://example.com/calendar.ics");
        assert_eq!(config.info_url, Some("https://example.com/info".to_string()));
        assert_eq!(config.reminders.len(), 2);
        assert_eq!(config.reminders[0].cron, "0 9 * * 1-5");
        assert_eq!(config.reminders[0].reminder_type, ReminderType::NextMeeting);
        assert_eq!(config.reminders[0].matrix_room, "!roomid:example.com");
        assert_eq!(config.reminders[1].cron, "0 8 * * 1");
        assert_eq!(config.reminders[1].reminder_type, ReminderType::AllUpcomingMeetings);
        assert_eq!(config.reminders[1].matrix_room, "!roomid:example.com");
        assert!(!config.bot_filtering.ignore_self);
        assert!(config.bot_filtering.ignore_bots);
        assert_eq!(config.bot_filtering.ignored_users.len(), 2);
        assert!(
            config
                .bot_filtering
                .ignored_users
                .contains(&"@spam-bot:example.com".to_string())
        );
        assert!(
            config
                .bot_filtering
                .ignored_users
                .contains(&"@announcement-bot:example.com".to_string())
        );
    }

    #[test]
    fn test_missing_homeserver_error() {
        // Given a TOML configuration missing the homeserver field
        let toml_str = indoc! {"
            username = \"@bot:example.com\"
            access_token = \"secret_token\"
            help_file = \"help.md\"
        "};

        // When parsing the TOML configuration
        let result = Config::from_toml(toml_str);

        // Then it should return an error indicating the missing field
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Missing 'homeserver'")
        );
    }

    #[test]
    fn test_missing_username_error() {
        // Given a TOML configuration missing the username field
        let toml_str = indoc! {"
            homeserver = \"https://matrix.example.com\"
            access_token = \"secret_token\"
            help_file = \"help.md\"
        "};

        // When parsing the TOML configuration
        let result = Config::from_toml(toml_str);

        // Then it should return an error indicating the missing field
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Missing 'username'")
        );
    }

    #[test]
    fn test_missing_access_token_error() {
        // Given a TOML configuration missing the access_token field
        let toml_str = indoc! {"
            homeserver = \"https://matrix.example.com\"
            username = \"@bot:example.com\"
            help_file = \"help.md\"
        "};

        // When parsing the TOML configuration
        let result = Config::from_toml(toml_str);

        // Then it should return an error indicating the missing field
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Missing 'access_token'")
        );
    }

    #[test]
    fn test_should_ignore_user_self_filtering() {
        // Given bot filtering config with ignore_self = true
        let config = BotFilteringConfig {
            ignore_self: true,
            ignore_bots: false,
            ignored_users: vec![],
        };
        let bot_user_id = "@help-bot:example.com";
        let other_user_id = "@user:example.com";

        // When checking if bot should ignore its own messages
        assert!(should_ignore_user(bot_user_id, bot_user_id, &config));
        // When checking if bot should ignore other user's messages
        assert!(!should_ignore_user(other_user_id, bot_user_id, &config));
    }

    #[test]
    fn test_should_ignore_user_bot_pattern() {
        // Given bot filtering config with ignore_bots = true
        let config = BotFilteringConfig {
            ignore_self: false,
            ignore_bots: true,
            ignored_users: vec![],
        };
        let bot_user_id = "@help-bot:example.com";
        let other_bot_id = "@spam-bot:example.com";
        let regular_user_id = "@user:example.com";

        // When checking different user types
        assert!(should_ignore_user(bot_user_id, bot_user_id, &config)); // contains "bot" even though ignore_self is false
        assert!(should_ignore_user(other_bot_id, bot_user_id, &config)); // contains "bot"
        assert!(!should_ignore_user(regular_user_id, bot_user_id, &config)); // doesn't contain "bot"
    }

    #[test]
    fn test_should_ignore_user_specific_list() {
        // Given bot filtering config with specific ignored users
        let config = BotFilteringConfig {
            ignore_self: false,
            ignore_bots: false,
            ignored_users: vec![
                "@spam-bot:example.com".to_string(),
                "@announcement-bot:example.com".to_string(),
            ],
        };
        let bot_user_id = "@help-bot:example.com";
        let spam_bot_id = "@spam-bot:example.com";
        let announcement_bot_id = "@announcement-bot:example.com";
        let regular_user_id = "@user:example.com";

        // When checking different users
        assert!(!should_ignore_user(bot_user_id, bot_user_id, &config));
        assert!(should_ignore_user(spam_bot_id, bot_user_id, &config));
        assert!(should_ignore_user(
            announcement_bot_id,
            bot_user_id,
            &config
        ));
        assert!(!should_ignore_user(regular_user_id, bot_user_id, &config));
    }

    #[test]
    fn test_should_ignore_user_case_insensitive() {
        // Given bot filtering config with ignore_bots = true
        let config = BotFilteringConfig {
            ignore_self: false,
            ignore_bots: true,
            ignored_users: vec![],
        };
        let bot_user_id = "@help-bot:example.com";
        let uppercase_bot_id = "@HELP-BOT:example.com";
        let mixed_case_bot_id = "@Help-Bot:example.com";

        // When checking case-insensitive bot detection
        assert!(should_ignore_user(uppercase_bot_id, bot_user_id, &config));
        assert!(should_ignore_user(mixed_case_bot_id, bot_user_id, &config));
    }
}
