//! Small UI-thread helpers: toasts, colors, external URLs, the macOS dock icon.

use std::sync::Mutex;
use std::time::Duration;

use slint::{ComponentHandle, SharedString};

use crate::PresenterWindow;

/// Locks a mutex, recovering the data if another thread panicked while holding it, so a crash
/// in a background media thread can't take the whole app down mid-presentation.
pub(crate) fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Shows a transient message in the presenter window (errors stay up longer).
pub(crate) fn show_toast(presenter: &PresenterWindow, message: impl Into<SharedString>, is_error: bool) {
    thread_local! {
        static HIDE_TIMER: slint::Timer = slint::Timer::default();
    }
    let message = message.into();
    if is_error {
        eprintln!("{}", message);
    }
    presenter.set_toast_text(message);
    presenter.set_toast_is_error(is_error);
    presenter.set_toast_visible(true);
    let weak = presenter.as_weak();
    let secs = if is_error { 8 } else { 4 };
    HIDE_TIMER.with(|t| {
        t.start(slint::TimerMode::SingleShot, Duration::from_secs(secs), move || {
            if let Some(p) = weak.upgrade() {
                p.set_toast_visible(false);
            }
        })
    });
}

pub(crate) fn to_slint_color(c: [u8; 4]) -> slint::Color {
    slint::Color::from_argb_u8(c[3], c[0], c[1], c[2])
}

pub(crate) fn open_external_url(url: &str) {
    let clean = url.trim();
    let url_to_open = if clean.starts_with("www.") {
        format!("https://{}", clean)
    } else {
        clean.to_string()
    };

    println!("Opening external URL: {}", url_to_open);
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open").arg(&url_to_open).spawn();
    }
    #[cfg(target_os = "windows")]
    {
        let _ = std::process::Command::new("cmd").args(["/C", "start", "", &url_to_open]).spawn();
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = std::process::Command::new("xdg-open").arg(&url_to_open).spawn();
    }
}

#[cfg(target_os = "macos")]
pub(crate) fn setup_macos_app_icon() {
    use std::ffi::{c_char, c_void};

    #[link(name = "AppKit", kind = "framework")]
    unsafe extern "C" {
        fn objc_getClass(name: *const c_char) -> *mut c_void;
        fn sel_registerName(name: *const c_char) -> *mut c_void;
        fn objc_msgSend();
    }

    let icon_bytes = include_bytes!("../../ui/icons/app_icon.png");

    unsafe {
        let cls_nsapp = objc_getClass(b"NSApplication\0".as_ptr() as *const c_char);
        let sel_shared_app = sel_registerName(b"sharedApplication\0".as_ptr() as *const c_char);
        let get_shared_app: unsafe extern "C" fn(*mut c_void, *mut c_void) -> *mut c_void =
            std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
        let app = get_shared_app(cls_nsapp, sel_shared_app);

        let cls_nsdata = objc_getClass(b"NSData\0".as_ptr() as *const c_char);
        let sel_data_with_bytes = sel_registerName(b"dataWithBytes:length:\0".as_ptr() as *const c_char);
        let make_data: unsafe extern "C" fn(*mut c_void, *mut c_void, *const u8, usize) -> *mut c_void =
            std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
        let data = make_data(cls_nsdata, sel_data_with_bytes, icon_bytes.as_ptr(), icon_bytes.len());

        let cls_nsimage = objc_getClass(b"NSImage\0".as_ptr() as *const c_char);
        let sel_alloc = sel_registerName(b"alloc\0".as_ptr() as *const c_char);
        let alloc_img: unsafe extern "C" fn(*mut c_void, *mut c_void) -> *mut c_void =
            std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
        let img_alloc = alloc_img(cls_nsimage, sel_alloc);

        let sel_init_with_data = sel_registerName(b"initWithData:\0".as_ptr() as *const c_char);
        let init_img: unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_void) -> *mut c_void =
            std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
        let image = init_img(img_alloc, sel_init_with_data, data);

        if !image.is_null() && !app.is_null() {
            let sel_set_icon = sel_registerName(b"setApplicationIconImage:\0".as_ptr() as *const c_char);
            let set_icon: unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_void) -> *mut c_void =
                std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
            set_icon(app, sel_set_icon, image);
            println!("macOS Dock icon successfully assigned to Presenter app icon.");
        }
    }
}
