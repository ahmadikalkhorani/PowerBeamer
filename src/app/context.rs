//! Handles shared by every group of UI callbacks.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use super::state::AppState;
use crate::media::InPlaceVideoPlayer;
use crate::{AudienceWindow, NotesWindow, PresenterWindow};

pub(crate) struct AppContext {
    pub(crate) state: Rc<RefCell<AppState>>,
    pub(crate) presenter_window: PresenterWindow,
    pub(crate) audience_window: AudienceWindow,
    pub(crate) notes_window: NotesWindow,
    pub(crate) update_ink_overlay: Rc<dyn Fn()>,
    pub(crate) video_player: Arc<Mutex<InPlaceVideoPlayer>>,
    pub(crate) audio_tick: Rc<RefCell<Option<Rc<super::scheduling::DemandTimer>>>>,
    pub(crate) update_slide_view: Rc<dyn Fn()>,
}
