use sysinfo::{System, SystemExt};

pub struct Time {
    days: u64,
    hours: u64,
    minutes: u64,
}

impl Time {
    pub fn uptime(sys: &System) -> Self {
        let uptime = sys.uptime();
        let days = uptime / 86400;
        let rem = uptime % 86400;
        let hours = rem / 3600;
        let rem = rem % 3600;
        let minutes = rem / 60;
        Self {
            days,
            hours,
            minutes,
        }
    }

    pub fn days(&self) -> u64 {
        self.days
    }

    pub fn hours(&self) -> u64 {
        self.hours
    }

    pub fn minutes(&self) -> u64 {
        self.minutes
    }
}
