//! Fixed windows short enough that a lost counter costs at most one window:
//! they stay in the state store, while longer calendar windows use the durable
//! quota tracker.

use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ShortWindow {
    Second,
    Minute,
    Hour,
}

impl ShortWindow {
    pub fn duration(&self) -> Duration {
        match self {
            ShortWindow::Second => Duration::from_secs(1),
            ShortWindow::Minute => Duration::from_secs(60),
            ShortWindow::Hour => Duration::from_secs(3600),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_windows_duration() {
        assert_eq!(ShortWindow::Second.duration(), Duration::from_secs(1));
        assert_eq!(ShortWindow::Minute.duration(), Duration::from_secs(60));
        assert_eq!(ShortWindow::Hour.duration(), Duration::from_secs(3600));
    }
}
