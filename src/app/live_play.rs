//! Play live: the Forge Player runs the open project from its data files, and the editor keeps
//! those files saved while it runs, so every edit shows in the game within about a second and
//! nothing is rebuilt. Custom Rust code still needs Preview (a real build).
//!
//! The other direction too: when the data files change outside the editor (an agent through the
//! MCP tools, a person in a text editor, a git checkout), the editor reloads them.

use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::time::{Duration, Instant};

use super::QuartzForgeApp;
use crate::core::store::ProjectStore;

/// Unsaved edits wait this long, so a slider drag writes a few times rather than every frame.
const SAVE_AFTER: Duration = Duration::from_millis(300);

/// How often the editor looks for changes made outside it.
const CHECK_DISK_EVERY: Duration = Duration::from_secs(1);

/// The running player, if any, and when the editor last wrote the project for it.
#[derive(Default)]
pub struct LivePlay {
    player: Option<Child>,
    dirty_since: Option<Instant>,
    /// The data files as the editor last wrote or read them.
    known_on_disk: Option<u64>,
    disk_checked_at: Option<Instant>,
}

impl Drop for LivePlay {
    /// The player closes with the editor rather than playing on, unwatched.
    fn drop(&mut self) {
        if let Some(mut child) = self.player.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

impl LivePlay {
    /// The editor wrote or read `root`: what is on disk now is what it knows.
    pub fn note_in_step(&mut self, root: &Path) {
        self.known_on_disk = Some(ProjectStore::new(root).fingerprint());
    }

    pub fn is_running(&mut self) -> bool {
        match &mut self.player {
            Some(child) => matches!(child.try_wait(), Ok(None)),
            None => false,
        }
    }

    /// The player binary: `QF_FORGE_PLAYER`, or `forge_player` beside the editor's own binary.
    /// `None` means it runs through cargo from the workspace instead.
    fn prebuilt_player() -> Option<PathBuf> {
        if let Some(path) = std::env::var_os("QF_FORGE_PLAYER") {
            return Some(PathBuf::from(path));
        }
        let beside = std::env::current_exe().ok()?.with_file_name("forge_player");
        beside.is_file().then_some(beside)
    }

    fn spawn(root: &Path) -> std::io::Result<Child> {
        if let Some(binary) = Self::prebuilt_player() {
            return Command::new(binary).arg(root).spawn();
        }
        let manifest = Path::new(env!("CARGO_MANIFEST_DIR")).join("../forge_player/Cargo.toml");
        Command::new("cargo")
            .args(["run", "--release", "--manifest-path"])
            .arg(manifest)
            .arg("--")
            .arg(root)
            .spawn()
    }
}

impl QuartzForgeApp {
    pub(super) fn start_live_play(&mut self) {
        let Some(root) = self.project_root.clone() else {
            self.status_line = "Open a project before playing it live.".to_owned();
            return;
        };
        if let Err(error) = ProjectStore::new(&root).save(&self.project_state.manifest) {
            self.status_line = format!("Play Live: could not write the project: {error:#}");
            return;
        }
        self.live_play.note_in_step(&root);
        match LivePlay::spawn(&root) {
            Ok(child) => {
                self.live_play.player = Some(child);
                self.status_line = format!("Playing {} live; edits show as you make them.", root.display());
            }
            Err(error) => self.status_line = format!("Play Live: could not start the Forge Player: {error}"),
        }
    }

    pub(super) fn stop_live_play(&mut self) {
        if let Some(mut child) = self.live_play.player.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        self.status_line = "Live play stopped.".to_owned();
    }

    /// Called every frame: while the player runs, write edits to the data files it watches.
    /// Only the data files are written (not the generated Rust), so this is cheap.
    pub(super) fn keep_live_files_saved(&mut self) {
        if !self.project_state.dirty || !self.live_play.is_running() {
            self.live_play.dirty_since = None;
            return;
        }
        let since = *self.live_play.dirty_since.get_or_insert_with(Instant::now);
        if since.elapsed() < SAVE_AFTER {
            return;
        }
        let Some(root) = self.project_root.clone() else { return };
        match ProjectStore::new(&root).save(&self.project_state.manifest) {
            Ok(_) => {
                self.live_play.dirty_since = None;
                self.live_play.note_in_step(&root);
            }
            Err(error) => self.status_line = format!("Play Live: could not write the project: {error:#}"),
        }
    }

    /// Called every frame: reload the project when its data files changed outside the editor.
    /// With unsaved edits it does not reload; it says so, and Save then keeps the editor's copy.
    pub(super) fn follow_disk_changes(&mut self) {
        let due = self.live_play.disk_checked_at.is_none_or(|at| at.elapsed() >= CHECK_DISK_EVERY);
        let Some(root) = self.project_root.clone() else { return };
        if !due {
            return;
        }
        self.live_play.disk_checked_at = Some(Instant::now());
        let now = ProjectStore::new(&root).fingerprint();
        let Some(known) = self.live_play.known_on_disk.replace(now) else { return };
        if known == now {
            return;
        }
        if self.project_state.dirty {
            self.status_line = "The project's files changed outside the editor while it has unsaved edits: Save keeps the editor's copy, Open Project takes theirs.".to_owned();
            return;
        }
        match crate::services::persistence::load_project(&root) {
            Ok(mut state) => {
                state.active_scene_index = self.project_state.active_scene_index.min(state.manifest.scenes.len().saturating_sub(1));
                self.project_state = state;
                self.quartz_preview = self.build_scene_source();
                self.status_line = format!("Reloaded {}: its files changed outside the editor.", root.display());
            }
            Err(error) => self.status_line = format!("The project's files changed but could not be read: {error:#}"),
        }
    }
}
