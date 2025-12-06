use anyhow::{Result, anyhow};
use ical::parser::ical::IcalParser;
use std::fs;
use std::io::BufReader;

pub struct CalendarEvent {
    pub summary: Option<String>,
    pub description: Option<String>,
    pub start_time: Option<String>,
    pub end_time: Option<String>,
    pub location: Option<String>,
    pub url: Option<String>,
}

pub struct IcalCalendar {
    pub events: Vec<CalendarEvent>,
}

impl IcalCalendar {
    pub fn from_file(file_path: &str) -> Result<Self> {
        let content = fs::read_to_string(file_path)?;
        Self::parse_ical_content(&content)
    }

    pub async fn from_url(url: &str) -> Result<Self> {
        let response = reqwest::get(url).await?;
        if !response.status().is_success() {
            return Err(anyhow!("HTTP error: {}", response.status()));
        }
        let content = response.text().await?;
        Self::parse_ical_content(&content)
    }

    pub fn from_url_blocking(url: &str) -> Result<Self> {
        let response = reqwest::blocking::get(url)?;
        if !response.status().is_success() {
            return Err(anyhow!("HTTP error: {}", response.status()));
        }
        let content = response.text()?;
        Self::parse_ical_content(&content)
    }

    fn parse_ical_content(content: &str) -> Result<Self> {
        let reader = BufReader::new(content.as_bytes());
        let parser = IcalParser::new(reader);

        let mut events = Vec::new();

        for calendar_result in parser {
            match calendar_result {
                Ok(calendar) => {
                    for event in calendar.events {
                        let mut calendar_event = CalendarEvent {
                            summary: None,
                            description: None,
                            start_time: None,
                            end_time: None,
                            location: None,
                            url: None,
                        };

                        for property in event.properties {
                            match property.name.as_str() {
                                "SUMMARY" => {
                                    calendar_event.summary = property.value.clone();
                                }
                                "DESCRIPTION" => {
                                    calendar_event.description = property.value.clone();
                                }
                                "DTSTART" => {
                                    calendar_event.start_time = property.value.clone();
                                }
                                "DTEND" => {
                                    calendar_event.end_time = property.value.clone();
                                }
                                "LOCATION" => {
                                    calendar_event.location = property.value.clone();
                                }
                                "URL" => {
                                    calendar_event.url = property.value.clone();
                                }
                                _ => {}
                            }
                        }

                        events.push(calendar_event);
                    }
                }
                Err(e) => {
                    return Err(anyhow!("Failed to parse iCal: {}", e));
                }
            }
        }

        Ok(IcalCalendar { events })
    }

    pub fn get_upcoming_events(&self, current_time: &str) -> Vec<&CalendarEvent> {
        self.get_upcoming_events_limited(current_time, None)
    }

    pub fn get_upcoming_events_limited(
        &self,
        current_time: &str,
        limit: Option<usize>,
    ) -> Vec<&CalendarEvent> {
        self.get_upcoming_events_filtered(current_time, None, limit)
    }

    pub fn get_upcoming_events_filtered(
        &self,
        current_time: &str,
        max_date: Option<&str>,
        limit: Option<usize>,
    ) -> Vec<&CalendarEvent> {
        let mut upcoming_events: Vec<&CalendarEvent> = self
            .events
            .iter()
            .filter(|event| {
                if let Some(start_time) = &event.start_time {
                    let start_time_str = start_time.as_str();
                    let is_future = start_time_str > current_time;

                    let is_before_max = if let Some(max_date) = max_date {
                        start_time_str <= max_date
                    } else {
                        true
                    };

                    is_future && is_before_max
                } else {
                    false
                }
            })
            .collect();

        upcoming_events.sort_by(|a, b| match (&a.start_time, &b.start_time) {
            (Some(a_time), Some(b_time)) => a_time.cmp(b_time),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => std::cmp::Ordering::Equal,
        });

        if let Some(limit) = limit {
            upcoming_events.truncate(limit);
        }

        upcoming_events
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use indoc::indoc;

    #[test]
    fn test_parse_ical_content() {
        let ical_content = indoc! {"
            BEGIN:VCALENDAR
            VERSION:2.0
            PRODID:-//Test//Test//EN
            BEGIN:VEVENT
            UID:test-event-1@example.com
            DTSTART:20251203T100000Z
            DTEND:20251203T110000Z
            SUMMARY:Test Meeting
            DESCRIPTION:This is a test meeting
            LOCATION:Conference Room
            END:VEVENT
            BEGIN:VEVENT
            UID:test-event-2@example.com
            DTSTART:20251204T140000Z
            DTEND:20251204T150000Z
            SUMMARY:Another Meeting
            END:VEVENT
            END:VCALENDAR
        "};

        let calendar = IcalCalendar::parse_ical_content(ical_content).unwrap();
        assert_eq!(calendar.events.len(), 2);

        let first_event = &calendar.events[0];
        assert_eq!(first_event.summary, Some("Test Meeting".to_string()));
        assert_eq!(
            first_event.description,
            Some("This is a test meeting".to_string())
        );
        assert_eq!(first_event.location, Some("Conference Room".to_string()));
        assert_eq!(first_event.start_time, Some("20251203T100000Z".to_string()));
        assert_eq!(first_event.end_time, Some("20251203T110000Z".to_string()));

        let second_event = &calendar.events[1];
        assert_eq!(second_event.summary, Some("Another Meeting".to_string()));
        assert_eq!(second_event.description, None);
        assert_eq!(second_event.location, None);
    }

    #[test]
    fn test_get_upcoming_events() {
        let ical_content = indoc! {"
            BEGIN:VCALENDAR
            VERSION:2.0
            PRODID:-//Test//Test//EN
            BEGIN:VEVENT
            UID:past-event@example.com
            DTSTART:20251201T100000Z
            DTEND:20251201T110000Z
            SUMMARY:Past Event
            END:VEVENT
            BEGIN:VEVENT
            UID:future-event@example.com
            DTSTART:20251205T100000Z
            DTEND:20251205T110000Z
            SUMMARY:Future Event
            END:VEVENT
            END:VCALENDAR
        "};

        let calendar = IcalCalendar::parse_ical_content(ical_content).unwrap();
        let upcoming = calendar.get_upcoming_events("20251203T120000Z");
        assert_eq!(upcoming.len(), 1);
        assert_eq!(upcoming[0].summary, Some("Future Event".to_string()));

        let upcoming_limited = calendar.get_upcoming_events_limited("20251203T120000Z", Some(1));
        assert_eq!(upcoming_limited.len(), 1);
        assert_eq!(
            upcoming_limited[0].summary,
            Some("Future Event".to_string())
        );
    }

    #[test]
    fn test_get_upcoming_events_limited_with_multiple() {
        let ical_content = indoc! {"
            BEGIN:VCALENDAR
            VERSION:2.0
            PRODID:-//Test//Test//EN
            BEGIN:VEVENT
            UID:past-event@example.com
            DTSTART:20251201T100000Z
            DTEND:20251201T110000Z
            SUMMARY:Past Event
            END:VEVENT
            BEGIN:VEVENT
            UID:future-event-1@example.com
            DTSTART:20251205T100000Z
            DTEND:20251205T110000Z
            SUMMARY:Future Event 1
            END:VEVENT
            BEGIN:VEVENT
            UID:future-event-2@example.com
            DTSTART:20251206T100000Z
            DTEND:20251206T110000Z
            SUMMARY:Future Event 2
            END:VEVENT
            BEGIN:VEVENT
            UID:future-event-3@example.com
            DTSTART:20251207T100000Z
            DTEND:20251207T110000Z
            SUMMARY:Future Event 3
            END:VEVENT
            END:VCALENDAR
        "};

        let calendar = IcalCalendar::parse_ical_content(ical_content).unwrap();

        let all_upcoming = calendar.get_upcoming_events("20251203T120000Z");
        assert_eq!(all_upcoming.len(), 3);

        let limited_upcoming = calendar.get_upcoming_events_limited("20251203T120000Z", Some(2));
        assert_eq!(limited_upcoming.len(), 2);
        assert_eq!(
            limited_upcoming[0].summary,
            Some("Future Event 1".to_string())
        );
        assert_eq!(
            limited_upcoming[1].summary,
            Some("Future Event 2".to_string())
        );

        let no_limit = calendar.get_upcoming_events_limited("20251203T120000Z", None);
        assert_eq!(no_limit.len(), 3);
    }

    #[test]
    fn test_get_upcoming_events_filtered_with_max_date() {
        let ical_content = indoc! {"
            BEGIN:VCALENDAR
            VERSION:2.0
            PRODID:-//Test//Test//EN
            BEGIN:VEVENT
            UID:past-event@example.com
            DTSTART:20251201T100000Z
            DTEND:20251201T110000Z
            SUMMARY:Past Event
            END:VEVENT
            BEGIN:VEVENT
            UID:near-future@example.com
            DTSTART:20251205T100000Z
            DTEND:20251205T110000Z
            SUMMARY:Near Future Event
            END:VEVENT
            BEGIN:VEVENT
            UID:far-future@example.com
            DTSTART:20251210T100000Z
            DTEND:20251210T110000Z
            SUMMARY:Far Future Event
            END:VEVENT
            END:VCALENDAR
        "};

        let calendar = IcalCalendar::parse_ical_content(ical_content).unwrap();

        let all_upcoming = calendar.get_upcoming_events("20251203T120000Z");
        assert_eq!(all_upcoming.len(), 2);

        let filtered = calendar.get_upcoming_events_filtered(
            "20251203T120000Z",
            Some("20251206T235959Z"),
            None,
        );
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].summary, Some("Near Future Event".to_string()));

        let filtered_limited = calendar.get_upcoming_events_filtered(
            "20251203T120000Z",
            Some("20251215T235959Z"),
            Some(1),
        );
        assert_eq!(filtered_limited.len(), 1);
        assert_eq!(
            filtered_limited[0].summary,
            Some("Near Future Event".to_string())
        );
    }
}
