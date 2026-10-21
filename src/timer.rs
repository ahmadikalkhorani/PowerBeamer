use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TimerStatus {
    Normal,
    Warning,
    Overtime,
}

/// `mm:ss`, or `h:mm:ss` from one hour on.
pub fn format_hms(total_secs: u64) -> String {
    let (h, m, s) = (total_secs / 3600, (total_secs / 60) % 60, total_secs % 60);
    if h > 0 {
        format!("{}:{:02}:{:02}", h, m, s)
    } else {
        format!("{:02}:{:02}", m, s)
    }
}

#[derive(Clone, Debug)]
pub struct PresentationTimer {
    pub start_time: Option<Instant>,
    pub accumulated: Duration,
    pub target_duration: Duration,
    pub is_running: bool,
}

impl PresentationTimer {
    pub fn new(target_minutes: u64) -> Self {
        Self {
            start_time: Some(Instant::now()),
            accumulated: Duration::ZERO,
            target_duration: Duration::from_secs(target_minutes * 60),
            is_running: true,
        }
    }

    pub fn elapsed(&self) -> Duration {
        if self.is_running {
            if let Some(start) = self.start_time {
                self.accumulated + start.elapsed()
            } else {
                self.accumulated
            }
        } else {
            self.accumulated
        }
    }

    pub fn elapsed_str(&self) -> String {
        format_hms(self.elapsed().as_secs())
    }

    pub fn remaining_str(&self) -> String {
        let elapsed_secs = self.elapsed().as_secs();
        let target_secs = self.target_duration.as_secs();
        if elapsed_secs >= target_secs {
            format!("+{}", format_hms(elapsed_secs - target_secs))
        } else {
            format_hms(target_secs - elapsed_secs)
        }
    }

    /// Time to show: elapsed for a stopwatch, remaining (then `+overtime`) for a countdown.
    pub fn display_str(&self, countdown: bool) -> String {
        if countdown {
            self.remaining_str()
        } else {
            self.elapsed_str()
        }
    }

    /// How close the talk is to its target: Normal, Warning once `warn_secs` or less remain
    /// (0 disables the warning), Overtime once the target is reached.
    pub fn status(&self, warn_secs: u64) -> TimerStatus {
        let elapsed = self.elapsed().as_secs();
        let target = self.target_duration.as_secs();
        if elapsed >= target {
            TimerStatus::Overtime
        } else if warn_secs > 0 && target - elapsed <= warn_secs {
            TimerStatus::Warning
        } else {
            TimerStatus::Normal
        }
    }

    pub fn set_target_minutes(&mut self, minutes: u64) {
        self.target_duration = Duration::from_secs(minutes * 60);
    }

    pub fn start(&mut self) {
        if !self.is_running {
            self.toggle();
        }
    }

    pub fn pause(&mut self) {
        if self.is_running {
            self.toggle();
        }
    }

    #[allow(dead_code)]
    pub fn format_display(&self) -> String {
        format!("{} / {}", self.elapsed_str(), self.remaining_str())
    }

    pub fn toggle(&mut self) {
        if self.is_running {
            if let Some(start) = self.start_time.take() {
                self.accumulated += start.elapsed();
            }
            self.is_running = false;
        } else {
            self.start_time = Some(Instant::now());
            self.is_running = true;
        }
    }

    pub fn reset(&mut self) {
        self.accumulated = Duration::ZERO;
        if self.is_running {
            self.start_time = Some(Instant::now());
        } else {
            self.start_time = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn timer_at(elapsed_secs: u64, target_min: u64) -> PresentationTimer {
        let mut t = PresentationTimer::new(target_min);
        t.pause();
        t.accumulated = Duration::from_secs(elapsed_secs);
        t
    }

    #[test]
    fn formats_minutes_and_hours() {
        assert_eq!(format_hms(65), "01:05");
        assert_eq!(format_hms(3_725), "1:02:05");
    }

    #[test]
    fn countdown_shows_remaining_then_overtime() {
        assert_eq!(timer_at(60, 10).display_str(true), "09:00");
        assert_eq!(timer_at(660, 10).display_str(true), "+01:00");
        assert_eq!(timer_at(660, 10).display_str(false), "11:00");
    }

    #[test]
    fn status_follows_warning_and_target() {
        assert_eq!(timer_at(200, 10).status(300), TimerStatus::Normal);
        assert_eq!(timer_at(300, 10).status(300), TimerStatus::Warning);
        assert_eq!(timer_at(599, 10).status(0), TimerStatus::Normal, "0 disables the warning");
        assert_eq!(timer_at(600, 10).status(300), TimerStatus::Overtime);
    }

    #[test]
    fn start_and_pause_are_idempotent() {
        let mut t = timer_at(30, 10);
        t.start();
        t.start();
        assert!(t.is_running);
        t.pause();
        t.pause();
        assert!(!t.is_running);
        assert!(t.elapsed().as_secs() >= 30);
    }
}
