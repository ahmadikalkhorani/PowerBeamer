//! GitHub release check, in-app updater and external links.


use slint::{ComponentHandle, SharedString};

use super::context::AppContext;
use super::ui::*;
use crate::updater::{self, UpdateInfo};

/// Wires updater callbacks and starts the background update check.
pub(crate) fn wire(ctx: &AppContext) -> Vec<slint::Timer> {
    let AppContext { presenter_window, .. } = ctx;

    // --- GITHUB AUTO-UPDATER & OPTIONS INTEGRATION ---
    presenter_window.set_app_version(SharedString::from(env!("CARGO_PKG_VERSION")));
    let update_info_cell = std::sync::Arc::new(std::sync::Mutex::new(None::<UpdateInfo>));

    {
        let p_weak = presenter_window.as_weak();
        let info_cell = update_info_cell.clone();
        presenter_window.on_check_for_updates(move || {
            if let Some(p) = p_weak.upgrade() {
                p.set_update_status(SharedString::from("checking"));
                p.set_update_status_message(SharedString::new());
            }
            let p_weak = p_weak.clone();
            let info_cell = info_cell.clone();
            std::thread::spawn(move || {
                let res = updater::check_for_update(updater::DEFAULT_REPO);
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(p) = p_weak.upgrade() {
                        match res {
                            Ok(Some(info)) => {
                                p.set_update_status(SharedString::from("available"));
                                p.set_latest_version(SharedString::from(&info.latest_version));
                                p.set_release_notes(SharedString::from(&info.release_notes));
                                p.set_release_url(SharedString::from(&info.release_url));
                                p.set_update_available_badge(true);
                                *lock(&info_cell) = Some(info);
                            }
                            Ok(None) => {
                                p.set_update_status(SharedString::from("up_to_date"));
                                p.set_update_available_badge(false);
                            }
                            Err(err) => {
                                p.set_update_status(SharedString::from("error"));
                                p.set_update_status_message(SharedString::from(err));
                            }
                        }
                    }
                });
            });
        });
    }

    {
        let p_weak = presenter_window.as_weak();
        let info_cell = update_info_cell.clone();
        presenter_window.on_install_update(move || {
            let maybe_info = lock(&info_cell).clone();
            if let Some(info) = maybe_info {
                if let Some(p) = p_weak.upgrade() {
                    p.set_update_status(SharedString::from("downloading"));
                    p.set_update_progress(0.0);
                }
                let p_weak = p_weak.clone();
                std::thread::spawn(move || {
                    let p_weak_progress = p_weak.clone();
                    let res = updater::download_and_install_update(&info.download_url, move |prog| {
                        let p_weak_progress = p_weak_progress.clone();
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(p) = p_weak_progress.upgrade() {
                                p.set_update_progress(prog);
                            }
                        });
                    });

                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(p) = p_weak.upgrade() {
                            match res {
                                Ok(()) => {
                                    p.set_update_status(SharedString::from("ready"));
                                }
                                Err(err) => {
                                    p.set_update_status(SharedString::from("error"));
                                    p.set_update_status_message(SharedString::from(err));
                                }
                            }
                        }
                    });
                });
            }
        });
    }

    presenter_window.on_restart_after_update(move || {
        updater::restart_application();
    });

    presenter_window.on_open_external_url(move |url| {
        open_external_url(url.as_str());
    });

    // Background non-blocking update check on startup
    {
        let p_weak = presenter_window.as_weak();
        let info_cell = update_info_cell.clone();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_secs(2));
            // Finish a half-applied update from an older updater before looking for new ones,
            // so the two never download/install at the same time.
            match updater::repair_stale_bundle(updater::DEFAULT_REPO) {
                Ok(true) => println!("Repaired app bundle; the complete update applies on next launch."),
                Ok(false) => {}
                Err(e) => eprintln!("Notice: couldn't repair app bundle: {}", e),
            }
            if let Ok(Some(info)) = updater::check_for_update(updater::DEFAULT_REPO) {
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(p) = p_weak.upgrade() {
                        p.set_update_status(SharedString::from("available"));
                        p.set_latest_version(SharedString::from(&info.latest_version));
                        p.set_release_notes(SharedString::from(&info.release_notes));
                        p.set_release_url(SharedString::from(&info.release_url));
                        p.set_update_available_badge(true);
                        *lock(&info_cell) = Some(info);
                    }
                });
            }
        });
    }

    Vec::new()
}
