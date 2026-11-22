//! Undo and redo for every project edit. A step is a snapshot of the project as JSON in parts
//! (the project header, each scene's settings, each object, event and logic tree); a part that
//! did not change is shared with the step before, so a step costs only what it changed.
//! The save time and the active scene are not edits and are left out.

use std::sync::Arc;

use anyhow::{Context, Result};

use crate::core::project::{ProjectManifest, SceneDocument};

#[derive(Debug, Clone)]
struct SceneParts {
    settings: Arc<str>,
    objects: Vec<Arc<str>>,
    events: Vec<Arc<str>>,
    logic_trees: Vec<Arc<str>>,
}

#[derive(Debug, Clone)]
pub(in crate::app) struct Snapshot {
    header: Arc<str>,
    scenes: Vec<SceneParts>,
}

impl Snapshot {
    fn capture(manifest: &mut ProjectManifest, previous: Option<&Self>) -> Self {
        let saved = std::mem::take(&mut manifest.last_saved_utc);
        let active = manifest.active_scene_id.take();
        let mut scenes = std::mem::take(&mut manifest.scenes);
        let header = Self::share(Self::text(manifest), previous.map(|snapshot| &snapshot.header));
        let parts = scenes
            .iter_mut()
            .enumerate()
            .map(|(index, scene)| Self::capture_scene(scene, previous.and_then(|snapshot| snapshot.scenes.get(index))))
            .collect();
        manifest.scenes = scenes;
        manifest.last_saved_utc = saved;
        manifest.active_scene_id = active;
        Self { header, scenes: parts }
    }

    fn capture_scene(scene: &mut SceneDocument, previous: Option<&SceneParts>) -> SceneParts {
        let view = Self::take_view(scene);
        let objects = std::mem::take(&mut scene.objects);
        let events = std::mem::take(&mut scene.events);
        let logic_trees = std::mem::take(&mut scene.logic_trees);
        let settings = Self::share(Self::text(scene), previous.map(|parts| &parts.settings));
        let parts = SceneParts {
            settings,
            objects: Self::share_all(&objects, previous.map(|parts| parts.objects.as_slice())),
            events: Self::share_all(&events, previous.map(|parts| parts.events.as_slice())),
            logic_trees: Self::share_all(&logic_trees, previous.map(|parts| parts.logic_trees.as_slice())),
        };
        (scene.objects, scene.events, scene.logic_trees) = (objects, events, logic_trees);
        (scene.canvas.zoom, scene.canvas.pan_x, scene.canvas.pan_y) = view;
        parts
    }

    /// Zoom and pan are where the user is looking, not an edit: kept out of the snapshot.
    fn take_view(scene: &mut SceneDocument) -> (f32, f32, f32) {
        let canvas = &mut scene.canvas;
        let view = (canvas.zoom, canvas.pan_x, canvas.pan_y);
        (canvas.zoom, canvas.pan_x, canvas.pan_y) = (0.0, 0.0, 0.0);
        view
    }

    fn share_all<T: serde::Serialize>(items: &[T], previous: Option<&[Arc<str>]>) -> Vec<Arc<str>> {
        items.iter().enumerate().map(|(index, item)| Self::share(Self::text(item), previous.and_then(|parts| parts.get(index)))).collect()
    }

    fn text<T: serde::Serialize>(value: &T) -> String {
        serde_json::to_string(value).unwrap_or_default()
    }

    /// The earlier part when its text is the same, so unchanged parts are stored once.
    fn share(text: String, previous: Option<&Arc<str>>) -> Arc<str> {
        match previous {
            Some(part) if **part == *text => Arc::clone(part),
            _ => Arc::from(text),
        }
    }

    fn same_as(&self, other: &Self) -> bool {
        let same = |a: &Arc<str>, b: &Arc<str>| Arc::ptr_eq(a, b) || a == b;
        let same_list = |a: &[Arc<str>], b: &[Arc<str>]| a.len() == b.len() && a.iter().zip(b).all(|(x, y)| same(x, y));
        same(&self.header, &other.header)
            && self.scenes.len() == other.scenes.len()
            && self.scenes.iter().zip(&other.scenes).all(|(a, b)| {
                same(&a.settings, &b.settings)
                    && same_list(&a.objects, &b.objects)
                    && same_list(&a.events, &b.events)
                    && same_list(&a.logic_trees, &b.logic_trees)
            })
    }

    /// The project this snapshot holds, keeping `current`'s save time and active scene.
    fn restore(&self, current: &ProjectManifest) -> Result<ProjectManifest> {
        let mut manifest: ProjectManifest = serde_json::from_str(&self.header).context("undo: project header")?;
        for parts in &self.scenes {
            let mut scene: SceneDocument = serde_json::from_str(&parts.settings).context("undo: scene settings")?;
            scene.objects = Self::parse_all(&parts.objects)?;
            scene.events = Self::parse_all(&parts.events)?;
            scene.logic_trees = Self::parse_all(&parts.logic_trees)?;
            let current_view = current.scenes.iter().find(|shown| shown.id == scene.id).map(|shown| &shown.canvas);
            let (zoom, pan_x, pan_y) = current_view.map_or((1.0, 0.0, 0.0), |canvas| (canvas.zoom, canvas.pan_x, canvas.pan_y));
            (scene.canvas.zoom, scene.canvas.pan_x, scene.canvas.pan_y) = (zoom, pan_x, pan_y);
            manifest.scenes.push(scene);
        }
        manifest.last_saved_utc.clone_from(&current.last_saved_utc);
        let active = current.active_scene_id.as_ref().filter(|id| manifest.scenes.iter().any(|scene| &scene.id == *id));
        manifest.active_scene_id = active.cloned().or_else(|| manifest.scenes.first().map(|scene| scene.id.clone()));
        Ok(manifest)
    }

    fn parse_all<T: serde::de::DeserializeOwned>(parts: &[Arc<str>]) -> Result<Vec<T>> {
        parts.iter().map(|part| serde_json::from_str(part).context("undo: project part")).collect()
    }
}

#[derive(Debug, Default)]
pub(in crate::app) struct History {
    /// Input arrived since the project was last compared.
    pub pending: bool,
    undo: Vec<Snapshot>,
    redo: Vec<Snapshot>,
    current: Option<Snapshot>,
}

impl History {
    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    pub fn has_baseline(&self) -> bool {
        self.current.is_some()
    }

    /// Forget every step: a different project was opened.
    pub fn reset(&mut self) {
        *self = Self { pending: true, ..Self::default() };
    }

    /// Record the project as it is now; a change since the last call becomes one undo step.
    pub fn observe(&mut self, manifest: &mut ProjectManifest) {
        let now = Snapshot::capture(manifest, self.current.as_ref());
        match self.current.take() {
            Some(before) if !before.same_as(&now) => {
                self.undo.push(before);
                self.redo.clear();
            }
            _ => {}
        }
        self.current = Some(now);
    }

    /// The project one step back, or `None` when there is nothing to undo.
    pub fn undo(&mut self, manifest: &ProjectManifest) -> Option<Result<ProjectManifest>> {
        let target = self.undo.pop()?;
        let restored = target.restore(manifest);
        if let Some(now) = self.current.replace(target) {
            self.redo.push(now);
        }
        Some(restored)
    }

    pub fn redo(&mut self, manifest: &ProjectManifest) -> Option<Result<ProjectManifest>> {
        let target = self.redo.pop()?;
        let restored = target.restore(manifest);
        if let Some(now) = self.current.replace(target) {
            self.undo.push(now);
        }
        Some(restored)
    }
}

#[cfg(test)]
mod tests {
    use super::History;
    use crate::core::project::ProjectManifest;
    use crate::core::quartz_domain::QuartzObjectBlueprint;

    #[test]
    fn an_edit_undoes_and_redoes_and_shares_unchanged_parts() {
        let mut manifest = ProjectManifest::new("undo".to_owned());
        manifest.scenes[0].objects = vec![
            QuartzObjectBlueprint::new("a".to_owned(), "a".to_owned()),
            QuartzObjectBlueprint::new("b".to_owned(), "b".to_owned()),
        ];
        let mut history = History::default();
        history.observe(&mut manifest);
        assert!(!history.can_undo());
        manifest.last_saved_utc = "later".to_owned();
        manifest.scenes[0].canvas.zoom = 0.25;
        history.observe(&mut manifest);
        assert!(!history.can_undo(), "saving and zooming are not edits");
        manifest.scenes[0].objects[1].x = 500.0;
        history.observe(&mut manifest);
        assert!(history.can_undo());
        let (before, now) = (&history.undo[0], history.current.as_ref().unwrap());
        assert!(std::sync::Arc::ptr_eq(&before.scenes[0].objects[0], &now.scenes[0].objects[0]), "object a unchanged, stored once");
        let undone = history.undo(&manifest).unwrap().unwrap();
        assert!((undone.scenes[0].objects[1].x - manifest.scenes[0].objects[1].x).abs() > 1.0);
        assert_eq!(undone.last_saved_utc, "later");
        assert!((undone.scenes[0].canvas.zoom - 0.25).abs() < f32::EPSILON, "undo keeps the view");
        let redone = history.redo(&undone).unwrap().unwrap();
        assert!((redone.scenes[0].objects[1].x - 500.0).abs() < f32::EPSILON);
        assert!(history.redo(&redone).is_none());
    }
}
