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
            let scene = self.make_scene("main_scene".to_owned(), SceneKind::Game);
            self.active_scene_id = Some(scene.id.clone());
            self.scenes.push(scene);
        }
    }

    pub fn make_scene(&mut self, name: String, kind: SceneKind) -> SceneDocument {
        let id = format!("scene_{:04}", self.next_scene_id);
        self.next_scene_id += 1;
        let source_file = format!("src/scripts/{}_scene.rs", name.replace(' ', "_").to_lowercase());
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
