//! Headless Slint scheduler measurement; simulated time, real timer callbacks.
#[allow(dead_code)]
#[path = "../src/app/scheduling.rs"]
mod scheduling;
use std::{cell::Cell, rc::Rc, time::Duration};
struct Clock(Rc<Cell<Duration>>);
impl slint::platform::Platform for Clock {
    fn create_window_adapter(
        &self,
    ) -> Result<Rc<dyn slint::platform::WindowAdapter>, slint::PlatformError> {
        Err("no windows in timer benchmark".into())
    }
    fn duration_since_start(&self) -> Duration {
        self.0.get()
    }
}
fn main() {
    let clock = Rc::new(Cell::new(Duration::ZERO));
    slint::platform::set_platform(Box::new(Clock(clock.clone()))).unwrap();
    let count = Rc::new(Cell::new(0u64));
    let timers: Vec<_> = [16, 16, 25, 50]
        .into_iter()
        .map(|ms| {
            let timer = slint::Timer::default();
            let count = count.clone();
            timer.start(
                slint::TimerMode::Repeated,
                Duration::from_millis(ms),
                move || count.set(count.get() + 1),
            );
            timer
        })
        .collect();
    for ms in 1..=10000 {
        clock.set(Duration::from_millis(ms));
        slint::platform::update_timers_and_animations();
    }
    let before = count.replace(0);
    drop(timers);
    let active = Rc::new(Cell::new(false));
    let demand: Vec<_> = [16, 16, 25]
        .into_iter()
        .map(|ms| {
            let active = active.clone();
            let count = count.clone();
            scheduling::DemandTimer::new(
                Duration::from_millis(ms),
                move || active.get(),
                move || count.set(count.get() + 1),
            )
        })
        .collect();
    for ms in 10001..=20000 {
        clock.set(Duration::from_millis(ms));
        slint::platform::update_timers_and_animations();
    }
    let after = count.replace(0);
    active.set(true);
    for timer in &demand {
        timer.wake();
    }
    for ms in 20001..=21000 {
        clock.set(Duration::from_millis(ms));
        slint::platform::update_timers_and_animations();
    }
    let active_ticks = count.replace(0);
    assert!(active_ticks > 100);
    active.set(false);
    for ms in 21001..=22000 {
        clock.set(Duration::from_millis(ms));
        slint::platform::update_timers_and_animations();
    }
    assert_eq!(count.get(), 0);
    assert!(slint::platform::duration_until_next_timer_update().is_none());
    let draws = Rc::new(Cell::new(0));
    let target = draws.clone();
    let render = scheduling::coalesce_frame(Rc::new(move || target.set(target.get() + 1)));
    for _ in 0..1000 {
        render();
    }
    clock.set(Duration::from_millis(22016));
    slint::platform::update_timers_and_animations();
    assert_eq!(draws.get(), 1);
    println!(
        "{{\"idle_10s_before_callbacks\":{before},\"idle_10s_after_callbacks\":{after},\"active_1s_callbacks\":{active_ticks},\"burst_input_events\":1000,\"burst_renders\":{}}}",
        draws.get()
    );
}
