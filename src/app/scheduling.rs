//! Timers that sleep completely between activities, and a frame-coalesced callback.
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    time::Duration,
};

pub(crate) struct DemandTimer {
    timer: slint::Timer,
    interval: Duration,
    active: Box<dyn Fn() -> bool>,
    tick: RefCell<Box<dyn FnMut()>>,
}
impl DemandTimer {
    pub fn new(
        interval: Duration,
        active: impl Fn() -> bool + 'static,
        tick: impl FnMut() + 'static,
    ) -> Rc<Self> {
        Rc::new(Self {
            timer: Default::default(),
            interval,
            active: Box::new(active),
            tick: RefCell::new(Box::new(tick)),
        })
    }
    pub fn wake(self: &Rc<Self>) {
        if self.timer.running() {
            return;
        }
        let weak = Rc::downgrade(self);
        self.timer
            .start(slint::TimerMode::Repeated, self.interval, move || {
                if let Some(t) = weak.upgrade() {
                    if !(t.active)() {
                        t.timer.stop();
                        return;
                    }
                    (t.tick.borrow_mut())();
                }
            });
    }
}

/// All input samples are retained; multiple events before a frame share one render.
pub(crate) fn coalesce_frame(render: Rc<dyn Fn()>) -> Rc<dyn Fn()> {
    let timer = Rc::new(slint::Timer::default());
    Rc::new(move || {
        if timer.running() {
            return;
        }
        let render = render.clone();
        timer.start(slint::TimerMode::SingleShot, Duration::ZERO, move || {
            render()
        });
    })
}

/// Flush once immediately before an actual window redraw, retaining the display's
/// native cadence (including high-refresh monitors). Software backends without a
/// notifier coalesce queued input with a zero-delay event-loop task instead.
pub(crate) fn coalesce_at_redraw(
    render: Rc<dyn Fn()>,
    windows: &[&slint::Window],
    request_redraw: Rc<dyn Fn()>,
) -> Rc<dyn Fn()> {
    let pending = Rc::new(Cell::new(false));
    let mut supported = true;
    for window in windows {
        let pending = pending.clone();
        let render = render.clone();
        supported &= window
            .set_rendering_notifier(move |phase, _| {
                if matches!(phase, slint::RenderingState::BeforeRendering) && pending.replace(false)
                {
                    render();
                }
            })
            .is_ok();
    }
    let fallback = {
        let pending = pending.clone();
        coalesce_frame(Rc::new(move || {
            if pending.replace(false) {
                render();
            }
        }))
    };
    Rc::new(move || {
        if pending.replace(true) {
            return;
        }
        request_redraw();
        if !supported {
            fallback();
        }
    })
}
