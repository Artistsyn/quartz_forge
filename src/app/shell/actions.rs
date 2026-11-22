//! What the menus, toolbar, hierarchy, palette and shortcuts do to the project, each in one
//! place so every way to reach an action behaves the same.

use std::path::Path;

use super::{CenterTab, DockTab, InspectorTab};
use crate::app::QuartzForgeApp;
use crate::core::project::{EditorProjectState, SceneKind};
use crate::services::persistence;

/// The kinds of object the Add menus create.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::app) enum NewObjectKind {
    Object,
    SpawnTemplate,
    Background,
}

impl NewObjectKind {
    pub const ALL: [Self; 3] = [Self::Object, Self::SpawnTemplate, Self::Background];

    pub fn menu_label(self) -> &'static str {
        match self {
            Self::Object => "Add Object",
            Self::SpawnTemplate => "Add Spawn Template",
            Self::Background => "Add Background Object",
        }
    }

    pub fn hint(self) -> &'static str {
        match self {
            Self::Object => "An object placed in the scene",
            Self::SpawnTemplate => "Not placed: copies are spawned by actions (bullets, pickups)",
            Self::Background => "A full-cell object drawn behind the scene",
        }
    }
}

impl QuartzForgeApp {
    pub(in crate::app) fn add_object_of(&mut self, kind: NewObjectKind) {
        let scene_index = self.project_state.active_scene_index;
        let Some(canvas) = self.project_state.manifest.scenes.get(scene_index).map(|scene| scene.canvas.clone()) else { return };
        match kind {
            NewObjectKind::Object => self.project_state.add_object_to_active_scene(),
            NewObjectKind::SpawnTemplate => self.project_state.add_spawn_only_object_to_active_scene(),
            NewObjectKind::Background => {
                let (cell_w, cell_h) = Self::background_cell_size(&canvas);
                self.project_state.add_background_object_to_active_scene(cell_w, cell_h);
            }
        }
        let grid = self.grid_size;
        let Some(scene) = self.project_state.manifest.scenes.get_mut(scene_index) else { return };
        if let Some(added) = scene.objects.last_mut() {
            if kind == NewObjectKind::Background {
                Self::apply_background_snap_if_needed(added, &canvas);
            } else if kind == NewObjectKind::Object && canvas.snap_to_grid {
                added.x = Self::snap_value(added.x, grid);
                added.y = Self::snap_value(added.y, grid);
            }
        }
        let last = scene.objects.len().saturating_sub(1);
        self.select_object(last);
        self.touch_and_refresh_preview();
    }

    pub(in crate::app) fn duplicate_selected_object(&mut self) {
        let scene_index = self.project_state.active_scene_index;
        let manifest = &mut self.project_state.manifest;
        let Some(scene_name) = manifest.scenes.get(scene_index).map(|scene| scene.name.clone()) else { return };
        let Some(mut copy) = manifest.scenes[scene_index].objects.get(self.selected_object_index).cloned() else { return };
        let (id, _) = manifest.next_object_identity(&scene_name);
        copy.name = format!("{} copy", copy.name);
        copy.id = id;
        copy.x += 24.0;
        copy.y += 24.0;
        let objects = &mut manifest.scenes[scene_index].objects;
        let at = (self.selected_object_index + 1).min(objects.len());
        self.status_line = format!("Duplicated as {}", copy.name);
        objects.insert(at, copy);
        self.select_object(at);
        self.touch_and_refresh_preview();
    }

    pub(in crate::app) fn delete_selected_object(&mut self) {
        let scene_index = self.project_state.active_scene_index;
        let Some(scene) = self.project_state.manifest.scenes.get_mut(scene_index) else { return };
        if self.selected_object_index >= scene.objects.len() {
            return;
        }
        let removed = scene.objects.remove(self.selected_object_index);
        self.selected_object_index = self.selected_object_index.min(scene.objects.len().saturating_sub(1));
        self.status_line = format!("Deleted {} (Undo brings it back)", removed.name);
        self.touch_and_refresh_preview();
    }

    pub(in crate::app) fn select_object(&mut self, index: usize) {
        self.selected_object_index = index;
        self.shell.tabs.inspector = InspectorTab::Object;
    }

    pub(in crate::app) fn select_scene(&mut self, index: usize) {
        self.project_state.set_active_scene(index);
        self.selected_object_index = 0;
        if let Some(scene) = self.project_state.manifest.scenes.get(index) {
            self.status_line = format!("Scene: {}", scene.name);
        }
    }

    pub(in crate::app) fn add_scene_named_next(&mut self) {
        let scenes = &self.project_state.manifest.scenes;
        let name = (scenes.len() + 1..).map(|n| format!("Scene {n}")).find(|name| !scenes.iter().any(|scene| &scene.name == name)).unwrap_or_default();
        self.project_state.add_scene(name.clone(), SceneKind::Game);
        self.select_scene(self.project_state.manifest.scenes.len().saturating_sub(1));
        self.status_line = format!("Added {name}");
    }

    pub(in crate::app) fn delete_active_scene(&mut self) {
        let index = self.project_state.active_scene_index;
        let name = self.project_state.manifest.scenes.get(index).map(|scene| scene.name.clone()).unwrap_or_default();
        self.project_state.remove_scene(index);
        self.selected_object_index = 0;
        self.status_line = format!("Deleted scene {name} (Undo brings it back)");
    }

    pub(in crate::app) fn toggle_live_play(&mut self) {
        if self.live_play.is_running() {
            self.stop_live_play();
        } else {
            self.start_live_play();
        }
    }

    pub(in crate::app) fn show_dock_tab(&mut self, tab: DockTab) {
        self.shell.tabs.dock = tab;
        self.shell.tabs.dock_open = true;
    }

    pub(in crate::app) fn show_center_tab(&mut self, tab: CenterTab) {
        if tab == CenterTab::Code {
            self.quartz_preview = self.build_scene_source();
        }
        self.shell.tabs.center = tab;
    }

    /// Open the project in `path`, reporting what went wrong on the status line.
    pub(in crate::app) fn open_project_at(&mut self, path: &Path) {
        match persistence::load_project_with_sync(path) {
            Ok((state, report)) => {
                self.adopt_project(state, path);
                self.show_project_sync_prompt = report.needs_user_action();
                self.project_sync_report = report.needs_user_action().then(|| report.clone());
                let name = self.project_state.manifest.project_name.clone();
                self.status_line = if report.needs_user_action() { format!("Opened {name}. {}", report.summary) } else { format!("Opened {name}") };
            }
            Err(error) => self.status_line = format!("Could not open {}: {error:#}", path.display()),
        }
    }

    /// Make `state` (loaded from or just created in `path`) the open project.
    pub(in crate::app) fn adopt_project(&mut self, state: EditorProjectState, path: &Path) {
        self.stop_live_play();
        self.project_state = state;
        self.live_play.note_in_step(path);
        self.project_root = Some(path.to_path_buf());
        self.quartz_preview = self.build_scene_source();
        self.project_sync_report = None;
        self.show_project_sync_prompt = false;
        self.show_startup_prompt = false;
        self.selected_object_index = 0;
        self.selected_event_index = 0;
        self.shell.history.reset();
        self.shell.framed_scene = None;
        let name = self.project_state.manifest.project_name.clone();
        if let Some(problem) = self.shell.recent.note_opened(&name, path) {
            self.status_line = problem;
        }
    }

    pub(in crate::app) fn close_project(&mut self) {
        self.stop_live_play();
        self.project_root = None;
        self.project_state = EditorProjectState::new("untitled_project".to_owned());
        self.show_startup_prompt = true;
        self.shell.history.reset();
        self.status_line = "Project closed.".to_owned();
    }

    pub(in crate::app) fn undo(&mut self) {
        let restored = self.shell.history.undo(&self.project_state.manifest);
        self.apply_restored(restored, "Undo");
    }

    pub(in crate::app) fn redo(&mut self) {
        let restored = self.shell.history.redo(&self.project_state.manifest);
        self.apply_restored(restored, "Redo");
    }

    fn apply_restored(&mut self, restored: Option<anyhow::Result<crate::core::project::ProjectManifest>>, what: &str) {
        match restored {
            None => self.status_line = format!("Nothing to {}", what.to_lowercase()),
            Some(Err(error)) => self.status_line = format!("{what} failed: {error:#}"),
            Some(Ok(manifest)) => {
                self.project_state.manifest = manifest;
                let active = self.project_state.manifest.active_scene_index().unwrap_or(0);
                self.project_state.set_active_scene(active);
                let objects = self.project_state.manifest.scenes.get(active).map_or(0, |scene| scene.objects.len());
                self.selected_object_index = self.selected_object_index.min(objects.saturating_sub(1));
                self.touch_and_refresh_preview();
                self.status_line = what.to_owned();
            }
        }
    }

    /// Record an undo step once the user has finished the edit in progress (no drag, no field
    /// being typed in). The project is only compared after input, not on idle repaints.
    pub(in crate::app) fn observe_history(&mut self, ctx: &eframe::egui::Context) {
        self.shell.history.pending |= ctx.input(|input| !input.events.is_empty()) || !self.shell.history.has_baseline();
        let busy = ctx.input(|input| input.pointer.any_down()) || ctx.memory(|memory| memory.focused().is_some());
        if self.shell.history.pending && !busy {
            self.shell.history.observe(&mut self.project_state.manifest);
            self.shell.history.pending = false;
        }
    }
}
