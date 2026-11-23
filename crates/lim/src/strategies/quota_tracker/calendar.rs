//! Calendar windows for durable quotas.
//!
//! Unlike the short fixed windows, which are plain durations, a long quota
//! follows the calendar in an explicit time zone: a day ends at local
//! midnight, a week starts on Monday, a month starts on its first day (or on a
//! configured billing day), and a year starts on 1 January. Daylight saving
//! therefore makes some days 23 or 25 hours long.
//!
//! The boundaries are epoch seconds: the ledger stores them, so a window never
//! depends on the process that computed it reading the same clock twice.

use jiff::{Span, Timestamp, Zoned, civil::Date, tz::TimeZone};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CalendarPeriod {
    Day,
    Week,
    Month,
    Year,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WindowBounds {
    /// First second of the window, inclusive.
    pub start: i64,
    /// First second after the window, exclusive.
    pub end: i64,
}

impl WindowBounds {
    pub(super) fn contains(self, now: i64) -> bool {
        self.start <= now && now < self.end
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum WindowError {
    #[error("timestamp is outside the supported calendar range")]
    Timestamp,
    #[error("the time zone has no local representation for this window boundary")]
    Boundary,
}

/// A calendar-aligned quota window in one time zone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CalendarWindow {
    period: CalendarPeriod,
    timezone: TimeZone,
    timezone_name: String,
    /// Day of the month a period starts on, restricted to 1..=28 so every
    /// month has it. Only meaningful for [`CalendarPeriod::Month`].
    anchor_day: Option<u8>,
}

impl CalendarWindow {
    /// Validate and build a window from its configuration.
    pub fn parse(
        period: CalendarPeriod,
        timezone: &str,
        anchor_day: Option<u8>,
    ) -> Result<Self, &'static str> {
        let parsed =
            TimeZone::get(timezone).map_err(|_| "timezone must be a valid IANA time zone name")?;
        match (period, anchor_day) {
            (CalendarPeriod::Month, Some(day)) if !(1..=28).contains(&day) => {
                return Err("anchor_day must be between 1 and 28");
            }
            (CalendarPeriod::Month, _) => {}
            (_, Some(_)) => return Err("anchor_day is only valid for the month period"),
            (_, None) => {}
        }
        Ok(Self {
            period,
            timezone: parsed,
            timezone_name: timezone.into(),
            anchor_day,
        })
    }

    pub fn period(&self) -> CalendarPeriod {
        self.period
    }

    pub(super) fn fingerprint(&self, limit: u64) -> String {
        let (period, anchor) = match self.period {
            CalendarPeriod::Day => ("day", 0),
            CalendarPeriod::Week => ("week", 0),
            CalendarPeriod::Month => ("month", self.anchor_day.unwrap_or(1)),
            CalendarPeriod::Year => ("year", 0),
        };
        let timezone = self.timezone.iana_name().unwrap_or(&self.timezone_name);
        format!("{limit}:{period}:{timezone}:{anchor}")
    }

    /// Resolve the window that contains `now_secs`.
    pub fn bounds(&self, now_secs: u64) -> Result<WindowBounds, WindowError> {
        let now =
            Timestamp::from_second(i64::try_from(now_secs).map_err(|_| WindowError::Timestamp)?)
                .map_err(|_| WindowError::Timestamp)?;
        let today = now.to_zoned(self.timezone.clone()).date();

        let (start, end) = match self.period {
            CalendarPeriod::Day => (today, tomorrow(today)?),
            CalendarPeriod::Week => {
                let monday_offset = i64::from(today.weekday().to_monday_zero_offset());
                let start = shift_days(today, -monday_offset)?;
                (start, shift_days(start, 7)?)
            }
            CalendarPeriod::Month => {
                let anchor = self.anchor_day.unwrap_or(1);
                let this_month = month_date(today.year(), today.month(), anchor)?;
                let start = if today.day() >= i8::try_from(anchor).expect("anchor is 1..=28") {
                    this_month
                } else {
                    let (year, month) = previous_month(today.year(), today.month());
                    month_date(year, month, anchor)?
                };
                let (year, month) = next_month(start.year(), start.month());
                (start, month_date(year, month, anchor)?)
            }
            CalendarPeriod::Year => {
                let start = year_date(today.year())?;
                (start, year_date(today.year() + 1)?)
            }
        };

        Ok(WindowBounds {
            start: local_midnight(start, &self.timezone)?
                .timestamp()
                .as_second(),
            end: local_midnight(end, &self.timezone)?.timestamp().as_second(),
        })
    }
}

fn tomorrow(date: Date) -> Result<Date, WindowError> {
    date.tomorrow().map_err(|_| WindowError::Timestamp)
}

fn shift_days(date: Date, days: i64) -> Result<Date, WindowError> {
    date.checked_add(Span::new().days(days))
        .map_err(|_| WindowError::Timestamp)
}

/// The anchor day exists in every month because configuration restricts it to
/// 1..=28.
fn month_date(year: i16, month: i8, anchor: u8) -> Result<Date, WindowError> {
    Date::new(year, month, i8::try_from(anchor).expect("anchor is 1..=28"))
        .map_err(|_| WindowError::Timestamp)
}

fn year_date(year: i16) -> Result<Date, WindowError> {
    Date::new(year, 1, 1).map_err(|_| WindowError::Timestamp)
}

fn next_month(year: i16, month: i8) -> (i16, i8) {
    if month == 12 {
        (year + 1, 1)
    } else {
        (year, month + 1)
    }
}

fn previous_month(year: i16, month: i8) -> (i16, i8) {
    if month == 1 {
        (year - 1, 12)
    } else {
        (year, month - 1)
    }
}

/// Midnight of `date` in `timezone`, in UTC.
///
/// A daylight-saving transition can skip or repeat local midnight. The
/// "compatible" disambiguation keeps the first occurrence of a repeated
/// midnight and moves a skipped one (for example in America/Santiago) to the
/// first second that exists on that local date.
fn local_midnight(date: Date, timezone: &TimeZone) -> Result<Zoned, WindowError> {
    timezone
        .to_ambiguous_zoned(date.into())
        .compatible()
        .map_err(|_| WindowError::Boundary)
}

#[cfg(test)]
mod tests {
    use super::*;
    use jiff::civil::{Time, Weekday};

    fn at(seconds: i64) -> u64 {
        u64::try_from(seconds).unwrap()
    }

    fn utc(year: i16, month: i8, day: i8) -> i64 {
        Date::new(year, month, day)
            .unwrap()
            .to_datetime(Time::midnight())
            .to_zoned(TimeZone::UTC)
            .unwrap()
            .timestamp()
            .as_second()
    }

    fn rome() -> CalendarWindow {
        CalendarWindow::parse(CalendarPeriod::Day, "Europe/Rome", None).unwrap()
    }

    #[test]
    fn a_day_follows_local_midnight_across_a_dst_change() {
        let window = rome();
        // 2026-03-29: clocks move from 02:00 CET to 03:00 CEST. The local day
        // is 23 hours long.
        let bounds = window.bounds(at(utc(2026, 3, 29))).unwrap();
        assert_eq!(bounds.start, utc(2026, 3, 29) - 3600);
        assert_eq!(bounds.end - bounds.start, 23 * 3600);
    }

    #[test]
    fn a_week_starts_on_monday() {
        let window = CalendarWindow::parse(CalendarPeriod::Week, "Europe/Rome", None).unwrap();
        let bounds = window.bounds(at(utc(2026, 11, 21))).unwrap();
        assert_eq!(
            Weekday::Saturday,
            Date::new(2026, 11, 21).unwrap().weekday()
        );
        // The week starts on Monday the 16th.
        assert_eq!(bounds.start, utc(2026, 11, 16) - 3600);
        assert_eq!(bounds.end, utc(2026, 11, 23) - 3600);
    }

    #[test]
    fn a_month_starts_on_the_first_day() {
        let window = CalendarWindow::parse(CalendarPeriod::Month, "Europe/Rome", None).unwrap();
        let bounds = window.bounds(at(utc(2026, 11, 21))).unwrap();
        assert_eq!(bounds.start, utc(2026, 11, 1) - 3600);
        assert_eq!(bounds.end, utc(2026, 12, 1) - 3600);
    }

    #[test]
    fn a_billing_month_uses_its_anchor_day() {
        let window = CalendarWindow::parse(CalendarPeriod::Month, "Europe/Rome", Some(15)).unwrap();
        let bounds = window.bounds(at(utc(2026, 11, 21))).unwrap();
        assert_eq!(bounds.start, utc(2026, 11, 15) - 3600);
        assert_eq!(bounds.end, utc(2026, 12, 15) - 3600);

        // Before the anchor the window still belongs to the previous month.
        let bounds = window.bounds(at(utc(2026, 11, 10))).unwrap();
        // 15 October is still CEST (UTC+2).
        assert_eq!(bounds.start, utc(2026, 10, 15) - 7200);
        assert_eq!(bounds.end, utc(2026, 11, 15) - 3600);
    }

    #[test]
    fn a_year_starts_on_the_first_of_january_locally() {
        let window = CalendarWindow::parse(CalendarPeriod::Year, "Europe/Rome", None).unwrap();
        let bounds = window.bounds(at(utc(2026, 11, 21))).unwrap();
        assert_eq!(bounds.start, utc(2026, 1, 1) - 3600);
        assert_eq!(bounds.end, utc(2027, 1, 1) - 3600);
    }

    #[test]
    fn time_zones_shift_the_window_start_in_utc() {
        let utc_window = CalendarWindow::parse(CalendarPeriod::Day, "UTC", None).unwrap();
        let tokyo = CalendarWindow::parse(CalendarPeriod::Day, "Asia/Tokyo", None).unwrap();
        let now = at(utc(2026, 11, 21) + 10 * 3600);
        assert_eq!(utc_window.bounds(now).unwrap().start, utc(2026, 11, 21));
        assert_eq!(
            tokyo.bounds(now).unwrap().start,
            utc(2026, 11, 20) + 15 * 3600
        );
    }

    #[test]
    fn invalid_time_zones_are_rejected() {
        assert!(CalendarWindow::parse(CalendarPeriod::Day, "Mars/Olympus", None).is_err());
        assert!(CalendarWindow::parse(CalendarPeriod::Day, "UTC", Some(1)).is_err());
        assert!(CalendarWindow::parse(CalendarPeriod::Month, "UTC", Some(0)).is_err());
        assert!(CalendarWindow::parse(CalendarPeriod::Month, "UTC", Some(29)).is_err());
        assert!(CalendarWindow::parse(CalendarPeriod::Month, "UTC", Some(28)).is_ok());
    }
}
