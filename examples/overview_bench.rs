//! Real Slint UI, headless software rendering, counting Rust heap allocations.
use slint::ComponentHandle;
use std::{
    alloc::{GlobalAlloc, Layout, System},
    rc::Rc,
    sync::atomic::{AtomicUsize, Ordering},
    time::Instant,
};
struct Alloc;
static LIVE: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);
unsafe impl GlobalAlloc for Alloc {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        let p = unsafe { System.alloc(l) };
        if !p.is_null() {
            let n = LIVE.fetch_add(l.size(), Ordering::Relaxed) + l.size();
            PEAK.fetch_max(n, Ordering::Relaxed);
        }
        p
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        LIVE.fetch_sub(l.size(), Ordering::Relaxed);
        unsafe { System.dealloc(p, l) };
    }
}
#[global_allocator]
static ALLOC: Alloc = Alloc;
slint::slint! { export { PresenterWindow, SlideThumbnail } from "ui/presenter.slint"; }
struct Headless;
impl slint::platform::Platform for Headless {
    fn create_window_adapter(
        &self,
    ) -> Result<Rc<dyn slint::platform::WindowAdapter>, slint::PlatformError> {
        Ok(
            slint::platform::software_renderer::MinimalSoftwareWindow::new(
                slint::platform::software_renderer::RepaintBufferType::NewBuffer,
            ),
        )
    }
}
fn main() {
    slint::platform::set_platform(Box::new(Headless)).unwrap();
    let before = LIVE.load(Ordering::Relaxed);
    let now = Instant::now();
    let p = PresenterWindow::new().unwrap();
    p.window().set_size(slint::PhysicalSize::new(1280, 800));
    p.set_total_slides(2000);
    p.set_show_thumbnails(true);
    let image =
        slint::Image::from_rgba8(slint::SharedPixelBuffer::<slint::Rgba8Pixel>::new(320, 180));
    p.set_slide_thumbnails(
        Rc::new(slint::VecModel::from(
            (1..=2000)
                .map(|index| SlideThumbnail {
                    index,
                    is_hidden: false,
                    image: image.clone(),
                })
                .collect::<Vec<_>>(),
        ))
        .into(),
    );
    p.show().unwrap();
    let pixels = p.window().take_snapshot().unwrap();
    std::hint::black_box(&pixels);
    println!(
        "{{\"slides\":2000,\"ms\":{:.3},\"heap_bytes\":{},\"peak_heap_bytes\":{}}}",
        now.elapsed().as_secs_f64() * 1000.,
        LIVE.load(Ordering::Relaxed) - before,
        PEAK.load(Ordering::Relaxed)
    );
}
