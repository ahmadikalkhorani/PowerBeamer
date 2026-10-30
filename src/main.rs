#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use slint::ComponentHandle;

mod app;
mod ink;
mod media;
mod pdf;
mod settings;
mod timer;
mod updater;

use media::InPlaceVideoPlayer;
use pdf::RenderQuality;
use settings::Settings;

slint::include_modules!();

use app::context::AppContext;
use app::gpu_images::*;
use app::state::*;
use app::ui::*;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Initializing Beamer LaTeX PDF Presenter...");

    let args: Vec<String> = std::env::args().collect();
    let saved_settings = Settings::load();
    let startup_quality = saved_settings
        .render_quality
        .as_deref()
        .map(RenderQuality::from_label)
        .unwrap_or(RenderQuality::Balanced);
    let mut startup_error: Option<String> = None;

    // A PDF given on the command line loads in the background once the window is up.
    let mut startup_path: Option<std::path::PathBuf> = None;
    if args.len() > 1 && !args[1].starts_with('-') {
        let path = std::path::PathBuf::from(&args[1]);
        if path.exists() {
            startup_path = Some(path);
        } else {
            startup_error = Some(format!(
                "File not found: {}. Showing the demo deck.",
                path.display()
            ));
        }
    }

    let mut initial_state = AppState::new();
    initial_state.apply_settings(&saved_settings);
    initial_state.deck.quality = startup_quality;
    let state = Rc::new(RefCell::new(initial_state));

    // Create main Presenter Window
    let presenter_window = PresenterWindow::new()?;

    // Create Audience Window (can be shown on external monitor / projector)
    let audience_window = AudienceWindow::new()?;

    // Create Detached Notes Window (can be shown on 3rd screen or tablet)
    let notes_window = NotesWindow::new()?;

    // Dynamic macOS Dock Icon Setup
    #[cfg(target_os = "macos")]
    setup_macos_app_icon();

    let update_ink_overlay =
        app::view::build_ink_overlay(&state, &presenter_window, &audience_window);

    // In-place video player: decodes frames directly into a Slint image (no external window)
    let video_player = Arc::new(Mutex::new(InPlaceVideoPlayer::new()));

    let update_slide_view = app::view::build_slide_view(
        &state,
        &presenter_window,
        &audience_window,
        &notes_window,
        &update_ink_overlay,
        &video_player,
    );

    // Initial sync
    state.borrow().push_settings_to_ui(&presenter_window);
    update_slide_view();
    if let Some(err) = startup_error.take() {
        show_toast(&presenter_window, err, true);
    }
    update_slide_thumbnails_model(&state.borrow().deck, &presenter_window);

    let ctx = AppContext {
        state: state.clone(),
        presenter_window: presenter_window.clone_strong(),
        audience_window: audience_window.clone_strong(),
        notes_window: notes_window.clone_strong(),
        update_ink_overlay: update_ink_overlay.clone(),
        video_player: video_player.clone(),
        update_slide_view: update_slide_view.clone(),
        audio_tick: Rc::new(RefCell::new(None)),
    };

    // Timers must stay alive for the whole run loop.
    let mut _timers: Vec<slint::Timer> = Vec::new();
    _timers.extend(app::session::wire(&ctx));
    _timers.extend(app::navigation::wire(&ctx));
    _timers.extend(app::tools::wire(&ctx));
    _timers.extend(app::pages::wire(&ctx));
    _timers.extend(app::media_controls::wire(&ctx));
    _timers.extend(app::canvas::wire(&ctx));
    _timers.extend(app::windows::wire(&ctx));
    _timers.extend(app::files::wire(&ctx));
    _timers.extend(app::updates::wire(&ctx));

    if let Some(path) = startup_path.take() {
        let name = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        state.borrow_mut().start_deck_load(path, DeckLoadKind::Open);
        presenter_window.set_deck_loading(true);
        show_toast(&presenter_window, format!("Opening {}…", name), false);
    }

    println!("Starting Beamer Presenter main window loop...");
    presenter_window.run()?;
    // Persist the final value even if the user exits before the next debounce tick.
    state.borrow().capture_settings().save_background();
    Settings::flush_saves();

    Ok(())
}

pub mod work;
