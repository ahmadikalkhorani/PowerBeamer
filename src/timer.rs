use std::time::{Duration, Instant};

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
        let total_secs = self.elapsed().as_secs();
        let mins = total_secs / 60;
        let secs = total_secs % 60;
        format!("{:02}:{:02}", mins, secs)
    }

    pub fn remaining_str(&self) -> String {
        let elapsed_secs = self.elapsed().as_secs();
        let target_secs = self.target_duration.as_secs();
        if elapsed_secs >= target_secs {
            let over = elapsed_secs - target_secs;
            format!("+{:02}:{:02}", over / 60, over % 60)
        } else {
            let rem = target_secs - elapsed_secs;
            format!("{:02}:{:02}", rem / 60, rem % 60)
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
