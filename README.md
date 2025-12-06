# Matrix Bot iCal

A Matrix bot that provides iCal/WebCal calendar integration with scheduled reminders.

## Features

- **Calendar Integration**: Fetches and parses iCal/WebCal calendars
- **Matrix Commands**: Responds to commands in Matrix rooms
- **Scheduled Reminders**: Cron-based automatic notifications
- **Human-readable Dates**: Converts iCal timestamps to readable format
- **Flexible Configuration**: Extensive TOML-based configuration

## Commands

- `!meeting` or `!event` - Shows the next upcoming meeting/event
- `!meetings` or `!events` - Shows all upcoming meetings/events

## Configuration

Create a `bot.toml` file based on `bot.toml.example`:

### Required Fields

```toml
homeserver = "https://matrix.example.com"
username = "@bot:example.com"
access_token = "secret_token"
webcal = "https://example.com/calendar.ics"
```

### Optional Fields

```toml
log_file = "/var/log/bot.log"
working_directory = "/app"
info_url = "https://example.com/info"

# Bot filtering configuration
[bot_filtering]
ignore_self = true
ignore_bots = false
ignored_users = ["@spam-bot:example.com"]

# Scheduled reminders
[[reminders]]
cron = "0 9 * * 1-5"  # 9:00 AM, Monday to Friday
reminder_type = "NextMeeting"
matrix_room = "!roomid:example.com"

[[reminders]]
cron = "0 8 * * 1"     # 8:00 AM, every Monday
reminder_type = "AllUpcomingMeetings"
matrix_room = "!roomid:example.com"
```

## Reminder Configuration

### Cron Format

Cron expressions use the format: `minute hour day-of-month month day-of-week`

Examples:
- `"0 9 * * 1-5"` - 9:00 AM, Monday to Friday
- `"0 8 * * 1"` - 8:00 AM, every Monday
- `"*/30 * * * *"` - Every 30 minutes
- `"0 0 1 * *"` - At midnight on the 1st of every month

It will also take English expressions using [english-to-cron](https://docs.rs/english-to-cron/latest/english_to_cron/fn.str_cron_syntax.html).

### Reminder Types

- `"NextMeeting"` - Sends only the next upcoming meeting/event
- `"AllUpcomingMeetings"` - Sends all upcoming meetings/events

## Installation

### From Source

```bash
git clone <repository-url>
cd matrix-bot-ical
cargo build --release
```

### Configuration

1. Copy `bot.toml.example` to `bot.toml`
2. Edit `bot.toml` with your Matrix server details and calendar URL
3. Ensure the bot user has access to the target rooms

### Running

```bash
# Run in foreground
./target/release/matrix-bot-ical

# Run as daemon
./target/release/matrix-bot-ical -d
```

## Development

### Building

```bash
cargo build
cargo test
```

### Testing

```bash
cargo test
```

## License

This project is dual-licensed under either:

- MIT License (LICENSE-MIT)
- Apache License 2.0 (LICENSE-APACHE)

## Example Usage

Once configured, the bot will:

1. Join rooms it's invited to
2. Respond to `!meeting`, `!event`, `!meetings`, and `!events` commands
3. Send scheduled reminders based on cron expressions
4. Format dates in human-readable format
5. Include info URLs when configured

Example output for `!meeting`:

```
# Next Meeting/Event

**Team Standup**
* Starts: Mon, Dec 09, 2025 at 09:00 AM
* Location: Conference Room A

For more information: https://example.com/info
```
