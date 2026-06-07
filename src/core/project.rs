use chrono::Utc;
use serde::{Deserialize, Serialize};

use crate::core::quartz_domain::{
    CustomCodeBlock, CustomCodeKind, LogicTree, QuartzEventBinding, QuartzObjectBlueprint,
    QuartzTargetRef, SceneCanvasSpec, SceneViewBookmark,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectManifest {
    pub format_version: u32,
    pub project_name: String,
    pub created_utc: String,
    pub last_saved_utc: String,
    pub next_scene_id: u32,
    #[serde(default = "default_next_object_id")]
    pub next_object_id: u32,
    #[serde(default = "default_next_logic_tree_id")]
    pub next_logic_tree_id: u32,
    #[serde(default = "default_next_event_id")]
    pub next_event_id: u32,
    #[serde(default = "default_next_custom_code_id")]
    pub next_custom_code_id: u32,
    pub active_scene_id: Option<String>,
    pub scenes: Vec<SceneDocument>,
    pub scripts: Vec<ScriptDocument>,
    pub plugins: Vec<String>,
    pub crystalline_enabled: bool,
}

impl ProjectManifest {
    pub fn new(project_name: String) -> Self {
        let now = Utc::now().to_rfc3339();
        let mut s = Self {
            format_version: 1,
            project_name,
            created_utc: now.clone(),
            last_saved_utc: now,
            next_scene_id: 1,
            next_object_id: 1,
            next_logic_tree_id: 1,
            next_event_id: 1,
            next_custom_code_id: 1,
            active_scene_id: None,
            scenes: Vec::new(),
            scripts: Vec::new(),
            plugins: Vec::new(),
            crystalline_enabled: true,
        };
        s.ensure_default_scene();
        s
    }

    pub fn ensure_default_scene(&mut self) {
        if self.scenes.is_empty() {
            let scene = self.make_scene("main".to_owned(), SceneKind::Game);
            self.active_scene_id = Some(scene.id.clone());
            self.scenes.push(scene);
        }
    }

    pub fn make_scene(&mut self, name: String, kind: SceneKind) -> SceneDocument {
        let id = format!("scene_{:04}", self.next_scene_id);
        self.next_scene_id += 1;
        let source_file = format!("src/scenes/{}_scene.rs", canonical_scene_slug(&name));
        SceneDocument {
            id,
            name,
            kind,
            source_file,
            notes: String::new(),
            canvas: SceneCanvasSpec::default(),
            objects: Vec::new(),
            logic_trees: Vec::new(),
            events: Vec::new(),
            custom_code_blocks: default_custom_code_blocks(),
            view_bookmarks: default_scene_view_bookmarks(),
        }
    }

    pub fn next_object_identity(&mut self, scene_name: &str) -> (String, String) {
        let id = format!("obj_{:04}", self.next_object_id);
        self.next_object_id += 1;
        let short = scene_name.replace(' ', "_").to_lowercase();
        let name = format!("{}_{}", short, id);
        (id, name)
    }

    pub fn next_logic_tree_identity(&mut self) -> (String, String) {
        let id = format!("logic_{:04}", self.next_logic_tree_id);
        self.next_logic_tree_id += 1;
        let name = format!("update_script_{}", self.next_logic_tree_id - 1);
        (id, name)
    }

    pub fn next_event_identity(&mut self) -> (String, String) {
        let id = format!("event_{:04}", self.next_event_id);
        self.next_event_id += 1;
        let name = format!("event_binding_{}", self.next_event_id - 1);
        (id, name)
    }

    pub fn next_custom_code_identity(&mut self, kind: CustomCodeKind) -> (String, String) {
        let id = format!("code_{:04}", self.next_custom_code_id);
        self.next_custom_code_id += 1;
        let name = format!("{}_{}", kind.as_str().to_lowercase(), self.next_custom_code_id - 1);
        (id, name)
    }

    pub fn touch_saved_time(&mut self) {
        self.last_saved_utc = Utc::now().to_rfc3339();
    }

    pub fn active_scene_index(&self) -> Option<usize> {
        let active_id = self.active_scene_id.as_deref()?;
        self.scenes.iter().position(|s| s.id == active_id)
    }
}

fn canonical_scene_slug(name: &str) -> String {
    let mut slug = name.trim().replace(' ', "_").to_lowercase();
    while slug.ends_with("_scene") {
        slug.truncate(slug.len().saturating_sub("_scene".len()));
    }
    if slug.is_empty() {
        "scene".to_owned()
    } else {
        slug
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SceneDocument {
    pub id: String,
    pub name: String,
    pub kind: SceneKind,
    pub source_file: String,
    pub notes: String,
    #[serde(default)]
    pub canvas: SceneCanvasSpec,
    #[serde(default)]
    pub objects: Vec<QuartzObjectBlueprint>,
    #[serde(default)]
    pub logic_trees: Vec<LogicTree>,
    #[serde(default)]
    pub events: Vec<QuartzEventBinding>,
    #[serde(default)]
    pub custom_code_blocks: Vec<CustomCodeBlock>,
    #[serde(default)]
    pub view_bookmarks: Vec<SceneViewBookmark>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SceneKind {
    Game,
    Ui,
    Cinematic,
    Test,
}

impl SceneKind {
    pub fn as_str(self) -> &'static str {
        match self {
            SceneKind::Game => "Game",
            SceneKind::Ui => "UI",
            SceneKind::Cinematic => "Cinematic",
            SceneKind::Test => "Test",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScriptDocument {
    pub name: String,
    pub rel_path: String,
}

#[derive(Debug, Clone)]
pub struct EditorProjectState {
    pub manifest: ProjectManifest,
    pub active_scene_index: usize,
    pub dirty: bool,
}

impl EditorProjectState {
    pub fn new(project_name: String) -> Self {
        let manifest = ProjectManifest::new(project_name);
        let active_scene_index = manifest.active_scene_index().unwrap_or(0);
        Self {
            manifest,
            active_scene_index,
            dirty: false,
        }
    }

    pub fn set_active_scene(&mut self, scene_index: usize) {
        if let Some(scene) = self.manifest.scenes.get(scene_index) {
            self.manifest.active_scene_id = Some(scene.id.clone());
            self.active_scene_index = scene_index;
        }
    }

    pub fn add_scene(&mut self, name: String, kind: SceneKind) {
        let scene = self.manifest.make_scene(name, kind);
        self.manifest.active_scene_id = Some(scene.id.clone());
        self.manifest.scenes.push(scene);
        self.active_scene_index = self.manifest.scenes.len().saturating_sub(1);
        self.dirty = true;
    }

    pub fn remove_scene(&mut self, scene_index: usize) {
        if self.manifest.scenes.len() <= 1 || scene_index >= self.manifest.scenes.len() {
            return;
        }
        self.manifest.scenes.remove(scene_index);
        let clamped = scene_index.min(self.manifest.scenes.len().saturating_sub(1));
        self.set_active_scene(clamped);
        self.dirty = true;
    }

    pub fn add_object_to_active_scene(&mut self) {
        let scene_name = self
            .manifest
            .scenes
            .get(self.active_scene_index)
            .map(|s| s.name.clone())
            .unwrap_or_else(|| "scene".to_owned());
        let scene_source_file = self
            .manifest
            .scenes
            .get(self.active_scene_index)
            .map(|s| s.source_file.clone())
            .unwrap_or_default();
        let (id, name) = self.manifest.next_object_identity(&scene_name);
        if let Some(scene) = self.manifest.scenes.get_mut(self.active_scene_index) {
            scene.objects.push(QuartzObjectBlueprint::new(id, name));
            if let Some(obj) = scene.objects.last_mut() {
                obj.output_file = scene_source_file;
            }
            self.dirty = true;
        }
    }

    pub fn add_background_object_to_active_scene(&mut self, cell_w: f32, cell_h: f32) {
        let scene_name = self
            .manifest
            .scenes
            .get(self.active_scene_index)
            .map(|s| s.name.clone())
            .unwrap_or_else(|| "scene".to_owned());
        let scene_source_file = self
            .manifest
            .scenes
            .get(self.active_scene_index)
            .map(|s| s.source_file.clone())
            .unwrap_or_default();
        let (id, name) = self.manifest.next_object_identity(&scene_name);
        if let Some(scene) = self.manifest.scenes.get_mut(self.active_scene_index) {
            let mut obj = QuartzObjectBlueprint::new(id, format!("{}_bg", name));
            obj.output_file = scene_source_file;
            obj.apply_background_defaults(cell_w, cell_h);
            scene.objects.push(obj);
            self.dirty = true;
        }
    }

    pub fn add_spawn_only_object_to_active_scene(&mut self) {
        let scene_name = self
            .manifest
            .scenes
            .get(self.active_scene_index)
            .map(|s| s.name.clone())
            .unwrap_or_else(|| "scene".to_owned());
        let scene_source_file = self
            .manifest
            .scenes
            .get(self.active_scene_index)
            .map(|s| s.source_file.clone())
            .unwrap_or_default();
        let (id, name) = self.manifest.next_object_identity(&scene_name);
        if let Some(scene) = self.manifest.scenes.get_mut(self.active_scene_index) {
            let mut obj = QuartzObjectBlueprint::new(id, format!("{}_spawn", name));
            obj.output_file = scene_source_file;
            obj.apply_spawn_only_defaults();
            scene.objects.push(obj);
            self.dirty = true;
        }
    }

    pub fn add_logic_tree_to_active_scene(&mut self) {
        let scene_source_file = self
            .manifest
            .scenes
            .get(self.active_scene_index)
            .map(|s| s.source_file.clone())
            .unwrap_or_default();
        let (id, name) = self.manifest.next_logic_tree_identity();
        if let Some(scene) = self.manifest.scenes.get_mut(self.active_scene_index) {
            let mut tree = LogicTree::new(id, name);
            tree.output_file = scene_source_file;
            scene.logic_trees.push(tree);
            self.dirty = true;
        }
    }

    pub fn add_event_binding_to_active_scene(&mut self) {
        let default_target = self
            .manifest
            .scenes
            .get(self.active_scene_index)
            .and_then(|scene| scene.objects.get(0))
            .map(|obj| QuartzTargetRef::Name(obj.id.clone()))
            .unwrap_or_else(|| QuartzTargetRef::Name("player".to_owned()));
        let scene_source_file = self
            .manifest
            .scenes
            .get(self.active_scene_index)
            .map(|s| s.source_file.clone())
            .unwrap_or_default();
        let (id, name) = self.manifest.next_event_identity();
        if let Some(scene) = self.manifest.scenes.get_mut(self.active_scene_index) {
            let mut binding = QuartzEventBinding::new(id, name, default_target);
            binding.output_file = scene_source_file;
            binding.refresh_references();
            scene.events.push(binding);
            self.dirty = true;
        }
    }

    pub fn add_custom_code_block_to_active_scene(&mut self, kind: CustomCodeKind) {
        let scene_source_file = self
            .manifest
            .scenes
            .get(self.active_scene_index)
            .map(|s| s.source_file.clone())
            .unwrap_or_default();
        let (id, name) = self.manifest.next_custom_code_identity(kind);
        let default_target = default_custom_code_target(&scene_source_file, kind);
        if let Some(scene) = self.manifest.scenes.get_mut(self.active_scene_index) {
            let mut block = CustomCodeBlock::new(id, name, kind, default_target);
            if matches!(kind, CustomCodeKind::CustomEvents) {
                block.custom_event_name = "custom_event".to_owned();
            }
            scene.custom_code_blocks.push(block);
            self.dirty = true;
        }
    }

    pub fn add_view_bookmark_to_active_scene(
        &mut self,
        name: String,
        pan_x: f32,
        pan_y: f32,
        zoom: f32,
    ) {
        let id = format!("bookmark_{}", Utc::now().timestamp_millis());
        if let Some(scene) = self.manifest.scenes.get_mut(self.active_scene_index) {
            scene.view_bookmarks.push(SceneViewBookmark {
                id,
                name,
                pan_x,
                pan_y,
                zoom,
            });
            self.dirty = true;
        }
    }

    pub fn preferred_scene_index_for_file(&self, rel_path: &str) -> Option<usize> {
        let rel_path = rel_path.trim();
        if rel_path.is_empty() || self.manifest.scenes.is_empty() {
            return None;
        }

        let mut matches = Vec::new();
        for (idx, scene) in self.manifest.scenes.iter().enumerate() {
            let scene_matches = scene.source_file == rel_path
                || scene.objects.iter().any(|obj| obj.output_file == rel_path)
                || scene.logic_trees.iter().any(|tree| tree.output_file == rel_path)
                || scene.events.iter().any(|event| event.output_file == rel_path)
                || scene.custom_code_blocks.iter().any(|block| block.output_file == rel_path);

            if scene_matches {
                matches.push(idx);
            }
        }

        if matches.is_empty() {
            Some(self.active_scene_index.min(self.manifest.scenes.len().saturating_sub(1)))
        } else if matches.contains(&self.active_scene_index) {
            Some(self.active_scene_index)
        } else {
            matches.into_iter().next()
        }
    }

    pub fn track_manual_override_for_file(&mut self, rel_path: &str, content: &str) -> Option<usize> {
        let scene_index = self.preferred_scene_index_for_file(rel_path)?;

        if let Some(scene) = self.manifest.scenes.get_mut(scene_index) {
            if let Some(existing) = scene
                .custom_code_blocks
                .iter_mut()
                .find(|b| b.kind == CustomCodeKind::ManualFileOverride && b.output_file == rel_path)
            {
                existing.code = content.to_owned();
                existing.name = format!("manual_override_{}", rel_path.replace('/', "_"));
                self.dirty = true;
                return Some(scene_index);
            }
        }

        let (id, name) = self
            .manifest
            .next_custom_code_identity(CustomCodeKind::ManualFileOverride);
        let mut block = CustomCodeBlock::new(
            id,
            name,
            CustomCodeKind::ManualFileOverride,
            rel_path.to_owned(),
        );
        block.code = content.to_owned();
        if let Some(scene) = self.manifest.scenes.get_mut(scene_index) {
            scene.custom_code_blocks.push(block);
            self.dirty = true;
            Some(scene_index)
        } else {
            None
        }
    }
}

fn default_next_object_id() -> u32 {
    1
}

fn default_next_logic_tree_id() -> u32 {
    1
}

fn default_next_event_id() -> u32 {
    1
}

fn default_next_custom_code_id() -> u32 {
    1
}

fn default_custom_code_target(scene_source_file: &str, kind: CustomCodeKind) -> String {
    match kind {
        CustomCodeKind::Constants => "src/constants.rs".to_owned(),
        CustomCodeKind::GameStateVars | CustomCodeKind::TypedVars => "src/game_state.rs".to_owned(),
        CustomCodeKind::CustomEvents
        | CustomCodeKind::UpdateLoops
        | CustomCodeKind::TopLevel
        | CustomCodeKind::ManualFileOverride => scene_source_file.to_owned(),
    }
}

fn default_custom_code_blocks() -> Vec<CustomCodeBlock> {
    vec![
        CustomCodeBlock::new(
            "code_defaults_constants".to_owned(),
            "constants".to_owned(),
            CustomCodeKind::Constants,
            "src/constants.rs".to_owned(),
        ),
        CustomCodeBlock::new(
            "code_defaults_game_state".to_owned(),
            "game_state".to_owned(),
            CustomCodeKind::GameStateVars,
            "src/game_state.rs".to_owned(),
        ),
    ]
}

fn default_scene_view_bookmarks() -> Vec<SceneViewBookmark> {
    vec![SceneViewBookmark::home_background_cell()]
}

#[cfg(test)]
mod tests {
    use super::EditorProjectState;
    use crate::core::quartz_domain::{CompareOp, QuartzCondition, QuartzExpr, QuartzExprKind, QuartzTargetRef};

    #[test]
    fn track_manual_override_prefers_active_scene_match() {
        let mut state = EditorProjectState::new("test_project".to_owned());
        state.manifest.scenes[0].source_file = "src/scripts/main_scene.rs".to_owned();

        let scene = state.manifest.make_scene("menu".to_owned(), super::SceneKind::Ui);
        state.manifest.active_scene_id = Some(scene.id.clone());
        state.manifest.scenes.push(scene);
        state.active_scene_index = 1;
        state.manifest.scenes[1].source_file = "src/scripts/menu_scene.rs".to_owned();

        let tracked = state.track_manual_override_for_file(
            "src/scripts/menu_scene.rs",
            "pub fn custom_menu_bits() {}",
        );

        assert_eq!(tracked, Some(1));
        assert!(state.manifest.scenes[1]
            .custom_code_blocks
            .iter()
            .any(|block| block.output_file == "src/scripts/menu_scene.rs" && block.code.contains("custom_menu_bits")));
    }

    #[test]
    fn track_manual_override_updates_existing_block() {
        let mut state = EditorProjectState::new("test_project".to_owned());
        let rel = "src/scripts/main_scene.rs";

        let first = state.track_manual_override_for_file(rel, "one");
        let second = state.track_manual_override_for_file(rel, "two");

        assert_eq!(first, Some(0));
        assert_eq!(second, Some(0));
        let overrides = state.manifest.scenes[0]
            .custom_code_blocks
            .iter()
            .filter(|block| block.output_file == rel)
            .collect::<Vec<_>>();
        assert_eq!(overrides.len(), 1);
        assert_eq!(overrides[0].code, "two");
    }

    #[test]
    fn condition_variants_serialize_roundtrip_compare() {
        let input = QuartzCondition::Compare {
            left: QuartzExpr {
                kind: QuartzExprKind::Var,
                raw: "score".to_owned(),
            },
            op: CompareOp::Ge,
            right: QuartzExpr {
                kind: QuartzExprKind::I32,
                raw: "10".to_owned(),
            },
        };

        let json = serde_json::to_string(&input).expect("compare condition should serialize");
        let restored: QuartzCondition =
            serde_json::from_str(&json).expect("compare condition should deserialize");

        match restored {
            QuartzCondition::Compare { left, op, right } => {
                assert_eq!(left.kind, QuartzExprKind::Var);
                assert_eq!(left.raw, "score");
                assert_eq!(op, CompareOp::Ge);
                assert_eq!(right.kind, QuartzExprKind::I32);
                assert_eq!(right.raw, "10");
            }
            other => panic!("unexpected condition variant after roundtrip: {:?}", other),
        }
    }

    #[test]
    fn condition_variants_serialize_roundtrip_planetary() {
        let input = QuartzCondition::DominantPlanetIs {
            target: QuartzTargetRef::Name("player".to_owned()),
            planet: QuartzTargetRef::Tag("planet".to_owned()),
        };

        let json = serde_json::to_string(&input).expect("planetary condition should serialize");
        let restored: QuartzCondition =
            serde_json::from_str(&json).expect("planetary condition should deserialize");

        match restored {
            QuartzCondition::DominantPlanetIs { target, planet } => {
                match target {
                    QuartzTargetRef::Name(name) => assert_eq!(name, "player"),
                    other => panic!("unexpected target variant: {:?}", other),
                }
                match planet {
                    QuartzTargetRef::Tag(tag) => assert_eq!(tag, "planet"),
                    other => panic!("unexpected planet variant: {:?}", other),
                }
            }
            other => panic!("unexpected condition variant after roundtrip: {:?}", other),
        }
    }

    #[test]
    fn condition_variants_serialize_roundtrip_emitter() {
        let input = QuartzCondition::EmitterActive {
            emitter: "thruster_smoke".to_owned(),
        };

        let json = serde_json::to_string(&input).expect("emitter condition should serialize");
        let restored: QuartzCondition =
            serde_json::from_str(&json).expect("emitter condition should deserialize");

        match restored {
            QuartzCondition::EmitterActive { emitter } => {
                assert_eq!(emitter, "thruster_smoke");
            }
            other => panic!("unexpected condition variant after roundtrip: {:?}", other),
        }
    }
}
