//! Keeping WoodSketch up to date. The app binary plugs in the actual updater (Velopack, checking
//! GitHub Releases); this side holds what it found and shows it: a quiet check at startup
//! (unless turned off), "Restart to update" once a new version is downloaded, and Help > Check
//! for Updates to ask on demand.

use std::sync::{Arc, Mutex};

/// What the updater is doing, or found.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub enum UpdateState {
    #[default]
    Idle,
    /// Run from a build, not installed: there's nothing to update in place.
    NotInstalled,
    Checking,
    UpToDate,
    /// Downloading this version.
    Downloading(String),
    /// This version is downloaded and installs on restart.
    Ready(String),
    Failed(String),
}

/// The updater the app binary provides.
pub struct Updater {
    pub state: Arc<Mutex<UpdateState>>,
    /// Check for an update in the background, downloading it if there is one.
    pub check: Box<dyn Fn() + Send + Sync>,
    /// Install the downloaded update and restart (the process exits).
    pub restart: Box<dyn Fn() + Send + Sync>,
}

impl Updater {
    pub fn state(&self) -> UpdateState {
        self.state.lock().map(|s| s.clone()).unwrap_or_default()
    }
}

/// The running version.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// What to say about an update for a check the user asked for (startup checks only speak up
/// when an update is ready).
pub fn describe(state: &UpdateState) -> String {
    match state {
        UpdateState::Idle | UpdateState::Checking => "Checking for updates...".into(),
        UpdateState::NotInstalled => "This copy wasn't installed with the WoodSketch installer, so it can't update itself. Download the latest from woodsketch.net.".into(),
        UpdateState::UpToDate => format!("WoodSketch {VERSION} is up to date."),
        UpdateState::Downloading(v) => format!("Downloading WoodSketch {v}..."),
        UpdateState::Ready(v) => format!("WoodSketch {v} is ready: restart to update."),
        UpdateState::Failed(e) => format!("Couldn't check for updates: {e}"),
    }
}
