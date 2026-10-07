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
            required_plugins: Vec::new(),
            pools: Vec::new(),
            camera: CameraSpec::default(),
            background: BackgroundSpec::default(),
            lighting: LightingSpec::default(),
            post_fx: PostFxSpec::default(),
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
    /// Quartz plugins this scene registers at setup. Plugins do NOT
    /// auto-register in the engine — a scene using Action::PluginCall or
    /// Action::RunPlugin without a matching registration compiles and then
    /// silently does nothing at runtime.
    #[serde(default)]
    pub required_plugins: Vec<PluginRegistration>,
    /// Pre-allocated object pools created at the end of setup_scene.
    #[serde(default)]
    pub pools: Vec<PoolBlueprint>,
    /// Runtime camera authoring (follow target, initial zoom).
    #[serde(default)]
    pub camera: CameraSpec,
    /// Composited full-screen background (generates a real background object).
    #[serde(default)]
    pub background: BackgroundSpec,
    /// SYNFUL-ONLY: real-time lighting + shadow authoring. Official quartz has
    /// no `lighting` module, so this never emits in the main-branch forge.
    #[serde(default)]
    pub lighting: LightingSpec,
    /// SYNFUL-ONLY: GPU post-processing stack (bloom + one post override).
    #[serde(default)]
    pub post_fx: PostFxSpec,
}

// ── SYNFUL lighting authoring ───────────────────────────────────────────────
//
// Ground truth: `arty/synful_quartz/quartz/src/lighting/types.rs` and
// `quartz/src/canvas/lighting_bridge.rs`. These types exist ONLY in the
// synful fork — the official quartz engine has no lighting module at all,
// which is why this whole surface lives in the synful copy of the forge.

/// Which kind of light. Mirrors `quartz::LightType`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum LightKindSpec {
    Point,
    Spot { direction: f32, cone_angle: f32 },
    Directional { dx: f32, dy: f32 },
}

impl Default for LightKindSpec {
    fn default() -> Self {
        LightKindSpec::Point
    }
}

impl LightKindSpec {
    pub const ALL: [&'static str; 3] = ["Point", "Spot", "Directional"];

    pub fn variant_name(&self) -> &'static str {
        match self {
            LightKindSpec::Point => "Point",
            LightKindSpec::Spot { .. } => "Spot",
            LightKindSpec::Directional { .. } => "Directional",
        }
    }
}

/// Per-light animation, ticked by the engine's `LightingSystem`.
/// Mirrors `quartz::LightEffect` plus a `None` case (the engine models the
/// absence as `Option<LightEffect>`; flattening it keeps the UI a single combo).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum LightEffectSpec {
    None,
    Pulse { min_intensity: f32, max_intensity: f32, speed: f32 },
    Flicker { base_intensity: f32, variance: f32 },
    ColorCycle { colors: Vec<[u8; 4]>, speed: f32 },
    FadeIn { target_intensity: f32, duration: f32 },
    FadeOut { duration: f32 },
}

impl Default for LightEffectSpec {
    fn default() -> Self {
        LightEffectSpec::None
    }
}

impl LightEffectSpec {
    pub const ALL: [&'static str; 6] =
        ["None", "Pulse", "Flicker", "ColorCycle", "FadeIn", "FadeOut"];

    pub fn variant_name(&self) -> &'static str {
        match self {
            LightEffectSpec::None => "None",
            LightEffectSpec::Pulse { .. } => "Pulse",
            LightEffectSpec::Flicker { .. } => "Flicker",
            LightEffectSpec::ColorCycle { .. } => "ColorCycle",
            LightEffectSpec::FadeIn { .. } => "FadeIn",
            LightEffectSpec::FadeOut { .. } => "FadeOut",
        }
    }
}

/// A single authored light. Emits as `LightSource::new(..)` plus the
/// builder/field mutations needed for the non-default parts.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LightSpec {
    pub id: String,
    #[serde(default)]
    pub kind: LightKindSpec,
    pub x: f32,
    pub y: f32,
    /// RGBA — `quartz::Color` is a 4-field tuple struct `Color(r, g, b, a)`.
    pub color: [u8; 4],
    pub radius: f32,
    pub intensity: f32,
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// `LightSource::new` defaults this to true.
    #[serde(default = "default_true")]
    pub casts_shadows: bool,
    #[serde(default)]
    pub effect: LightEffectSpec,
    /// When non-empty, emits `canvas.attach_light(id, object, offset)` so the
    /// light follows that object every frame.
    #[serde(default)]
    pub attach_object: String,
    #[serde(default)]
    pub attach_offset_x: f32,
    #[serde(default)]
    pub attach_offset_y: f32,
}

impl LightSpec {
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            kind: LightKindSpec::Point,
            x: 0.0,
            y: 0.0,
            color: [255, 220, 150, 255],
            radius: 350.0,
            intensity: 0.7,
            enabled: true,
            casts_shadows: true,
            effect: LightEffectSpec::None,
            attach_object: String::new(),
            attach_offset_x: 0.0,
            attach_offset_y: 0.0,
        }
    }
}

/// Scene-level lighting. Emits `canvas.enable_lighting(LightingConfig { .. })`
/// followed by one `canvas.add_light(..)` per light.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LightingSpec {
    pub enabled: bool,
    /// Ambient is what makes lights readable — at strength 1.0 everything is
    /// already fully lit and individual lights are invisible.
    pub ambient_color: [u8; 4],
    pub ambient_strength: f32,
    pub max_lights: usize,
    pub lights: Vec<LightSpec>,
}

impl Default for LightingSpec {
    fn default() -> Self {
        Self {
            enabled: false,
            // Matches `AmbientLight::dark()` — the preset that makes authored
            // lights actually visible.
            ambient_color: [10, 10, 25, 255],
            ambient_strength: 0.06,
            max_lights: 64,
            lights: Vec::new(),
        }
    }
}

/// The single active post-processing override. The engine holds ONE
/// `active_post_override` at a time, so this is an enum rather than a set of
/// independent toggles — modelling it as flags would let the UI express a
/// combination the engine silently collapses.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum PostFxMode {
    None,
    Vignette { strength: f32, radius: f32, softness: f32 },
    ChromaticAberration { intensity: f32 },
    NightMode {
        bloom_threshold: f32,
        bloom_strength: f32,
        vignette_strength: f32,
        vignette_radius: f32,
        vignette_softness: f32,
        ca_intensity: f32,
    },
    /// Author-supplied WGSL registered via `register_shader_source` then
    /// activated with `set_post_override`.
    Custom { shader_id: String, label: String, wgsl: String, params: Vec<f32> },
}

impl Default for PostFxMode {
    fn default() -> Self {
        PostFxMode::None
    }
}

impl PostFxMode {
    pub const ALL: [&'static str; 5] =
        ["None", "Vignette", "ChromaticAberration", "NightMode", "Custom"];

    pub fn variant_name(&self) -> &'static str {
        match self {
            PostFxMode::None => "None",
            PostFxMode::Vignette { .. } => "Vignette",
            PostFxMode::ChromaticAberration { .. } => "ChromaticAberration",
            PostFxMode::NightMode { .. } => "NightMode",
            PostFxMode::Custom { .. } => "Custom",
        }
    }
}

/// GPU post-processing. Bloom is a separate pass from the post override, so
/// it composes with any mode (that is why it is a sibling field, not a variant).
///
/// Deliberately excluded: `enable_air_barrier`. Its 11 arguments are per-frame
/// gameplay values (time, player speed, player screen UV, facing direction) —
/// it is a runtime effect, not scene authoring, and belongs in custom code.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PostFxSpec {
    pub bloom_enabled: bool,
    pub bloom_threshold: f32,
    pub bloom_strength: f32,
    pub mode: PostFxMode,
}

impl Default for PostFxSpec {
    fn default() -> Self {
        Self {
            bloom_enabled: false,
            // Matches `BloomSettings::default()`.
            bloom_threshold: 0.8,
            bloom_strength: 0.4,
            mode: PostFxMode::None,
        }
    }
}

/// Runtime camera authoring for a scene. Emitted into setup_scene via
/// `canvas.camera_mut()`. Canvas::new installs a default camera, so
/// `camera_mut()` is Some at setup time.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CameraSpec {
    /// Emit `cam.follow(Some(Target::...))` when true; otherwise nothing.
    #[serde(default)]
    pub follow_enabled: bool,
    #[serde(default = "default_follow_target")]
    pub follow_target: QuartzTargetRef,
    /// Initial zoom applied at setup. 1.0 = no change.
    #[serde(default = "default_camera_zoom")]
    pub initial_zoom: f32,
    /// true → `cam.smooth_zoom(z)`, false → `cam.snap_zoom(z)`. Only emitted
    /// when initial_zoom differs from 1.0.
    #[serde(default)]
    pub smooth_initial_zoom: bool,
}

fn default_follow_target() -> QuartzTargetRef {
    QuartzTargetRef::Name("player".to_owned())
}
fn default_camera_zoom() -> f32 {
    1.0
}

impl Default for CameraSpec {
    fn default() -> Self {
        Self {
            follow_enabled: false,
            follow_target: default_follow_target(),
            initial_zoom: 1.0,
            smooth_initial_zoom: false,
        }
    }
}

/// A composited full-screen background. Generates a real `.unlit()` background
/// GameObject whose image is built from a `LayeredBackground` — this RENDERS
/// (unlike BackgroundPlugin, which composites but has no draw hook and is used
/// by no game). Layers map 1:1 to quartz `BackgroundLayer` variants.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BackgroundSpec {
    #[serde(default)]
    pub enabled: bool,
    /// Generated background object id.
    #[serde(default = "default_background_object_id")]
    pub object_id: String,
    /// Render layer — should be well below gameplay (default -100).
    #[serde(default = "default_background_layer")]
    pub render_layer: i32,
    /// Pin to the camera (screen-space) vs. world-space.
    #[serde(default = "default_true")]
    pub camera_pinned: bool,
    /// Global tint applied after compositing. [255,255,255] = identity.
    #[serde(default = "default_white_tint")]
    pub tint: [u8; 3],
    #[serde(default)]
    pub layers: Vec<BackgroundLayerSpec>,
    /// A PathForge background instead of layers: the `quartz_path_forge`
    /// plugin walks a PathForge scene or journey onto the background object
    /// (live, or from exported frames). When set, the layers are not emitted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path_forge: Option<PathForgeBackground>,
    /// When true, author the background THROUGH `BackgroundPlugin`: the plugin
    /// composites + disk-caches the image (heavy starfield/nebula composites
    /// are built once and loaded from disk on later launches), then the
    /// composited image is pulled via `current_image()` onto the background
    /// object — the image enters the game through existing API, the plugin is
    /// not relied on to render. When false, the composite is built inline via
    /// `LayeredBackground::build()` every launch (no cache).
    #[serde(default)]
    pub use_plugin_cache: bool,
    /// Disk cache directory (relative to project root) for plugin-cache mode.
    #[serde(default = "default_bg_cache_dir")]
    pub cache_dir: String,
    /// Primary background key registered with the plugin (plugin-cache mode).
    #[serde(default = "default_bg_key")]
    pub background_key: String,
    /// Additional named backgrounds beyond the primary (plugin-cache mode).
    /// Enables runtime switching / crossfade transitions between them.
    #[serde(default)]
    pub backgrounds: Vec<NamedBackground>,
    /// Key shown at setup. Empty → the primary (`background_key`).
    #[serde(default)]
    pub active_key: String,
    /// Emit a per-frame `on_update` that pulls `current_image()` onto the
    /// background object. Required for crossfade transitions to blend on
    /// screen (the plugin computes the blend in its own on_update; without a
    /// per-frame pull only the setup-time frame is shown). Plugin-cache only.
    #[serde(default)]
    pub per_frame_pull: bool,
}

impl BackgroundSpec {
    /// Enabled and drawn by the PathForge plugin.
    pub fn path_forge_active(&self) -> Option<&PathForgeBackground> {
        if self.enabled { self.path_forge.as_ref() } else { None }
    }

    /// Enabled and composited from layers (not a PathForge background).
    pub fn layered_active(&self) -> bool {
        self.enabled && self.path_forge.is_none() && !self.layers.is_empty()
    }

    /// All backgrounds to register, primary first: (key, tint, layers).
    pub fn resolved_backgrounds(&self) -> Vec<NamedBackground> {
        let mut out = vec![NamedBackground {
            key: self.background_key.clone(),
            tint: self.tint,
            layers: self.layers.clone(),
        }];
        for bg in &self.backgrounds {
            if bg.key != self.background_key {
                out.push(bg.clone());
            }
        }
        out
    }

    /// The key shown at setup (active_key, falling back to the primary).
    pub fn effective_active_key(&self) -> String {
        let k = self.active_key.trim();
        if k.is_empty() {
            self.background_key.clone()
        } else {
            k.to_owned()
        }
    }
}

fn default_bg_cache_dir() -> String {
    "assets/bg_cache".to_owned()
}
fn default_bg_key() -> String {
    "main".to_owned()
}

fn default_background_object_id() -> String {
    "background".to_owned()
}
fn default_background_layer() -> i32 {
    -100
}
fn default_true() -> bool {
    true
}
fn default_white_tint() -> [u8; 3] {
    [255, 255, 255]
}

impl Default for BackgroundSpec {
    fn default() -> Self {
        Self {
            enabled: false,
            object_id: default_background_object_id(),
            render_layer: default_background_layer(),
            camera_pinned: true,
            tint: [255, 255, 255],
            layers: Vec::new(),
            path_forge: None,
            use_plugin_cache: false,
            cache_dir: default_bg_cache_dir(),
            background_key: default_bg_key(),
            backgrounds: Vec::new(),
            active_key: String::new(),
            per_frame_pull: false,
        }
    }
}

/// A PathForge background (see `BackgroundSpec::path_forge`). Generated as
/// `PathForgePlugin::live(..)` or `::frames(..)` from the `quartz_path_forge`
/// crate, dispatched as `Action::RunPlugin { name: "path_forge", data }`
/// (`walk`, `stop`, `speed:<m/s>`, `next`, `choose:left|right`, `finish`, …).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PathForgeBackground {
    /// Relative to the project root: a PathForge scene `.json` or a
    /// `.journey.json` (live), or what `pf journey --formats png` /
    /// `pf export --formats png` wrote (frames). Live also takes `preset:<Name>`.
    pub source: String,
    #[serde(default)]
    pub mode: PathForgeMode,
    /// Live: pixels rendered per frame (the object scales them to its size).
    #[serde(default = "default_path_forge_size")]
    pub size: [u32; 2],
    /// Walking speed in m/s; None walks at each scene's own speed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speed: Option<f32>,
    /// Live: frames rendered per second at most.
    #[serde(default = "default_path_forge_fps")]
    pub render_fps: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum PathForgeMode {
    /// PathForge renders every frame in the game, on a worker thread.
    #[default]
    Live,
    /// Plays exported PNG frames; nothing is rendered on the device.
    Frames,
}

impl PathForgeBackground {
    pub fn new(source: impl Into<String>) -> Self {
        Self { source: source.into(), mode: PathForgeMode::Live, size: default_path_forge_size(), speed: None, render_fps: default_path_forge_fps() }
    }
}

fn default_path_forge_size() -> [u32; 2] {
    [270, 480]
}
fn default_path_forge_fps() -> f32 {
    30.0
}

/// An additional named background beyond the primary. Registered with the
/// plugin via `set_background(key, ...)`; switch/crossfade to it at runtime
/// with `Action::RunPlugin { name: "background", data: "set:key" | "transition:from,to,dur" }`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NamedBackground {
    pub key: String,
    #[serde(default = "default_white_tint")]
    pub tint: [u8; 3],
    pub layers: Vec<BackgroundLayerSpec>,
}

/// One background layer — mirrors `quartz::plugin::background::BackgroundLayer`.
/// Colors are u8 triples (integer literals are correct for these); f32 fields
/// (nebula density, scale) must be emitted via f32_lit.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum BackgroundLayerSpec {
    Solid { color: [u8; 3] },
    GradientVertical { top: [u8; 3], bottom: [u8; 3] },
    GradientHorizontal { left: [u8; 3], right: [u8; 3] },
    GradientFourCorner {
        top_left: [u8; 3],
        top_right: [u8; 3],
        bottom_left: [u8; 3],
        bottom_right: [u8; 3],
    },
    Starfield {
        density: u32,
        seed: u64,
        size_min: u32,
        size_max: u32,
        brightness_min: u8,
        brightness_max: u8,
        vertical_fade: Option<u32>,
    },
    Nebula {
        color: [u8; 3],
        density: f32,
        seed: u64,
    },
    /// An image asset composited into the background, resized to the background
    /// dimensions with the chosen filter. Emits
    /// `BackgroundLayer::Image { bytes: include_bytes!(..), filter: ResizeFilter::X }`.
    /// (Enabled once quartz re-exported `ResizeFilter` from the background
    /// module — before that the variant was unconstructable downstream.)
    Image {
        /// Path relative to the project root, e.g. "assets/sky.png".
        asset_path: String,
        #[serde(default)]
        filter: BackgroundResizeFilter,
    },
}

/// Mirrors `quartz::plugin::background::ResizeFilter`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum BackgroundResizeFilter {
    /// Crisp pixels — pixel art.
    Nearest,
    /// Fast and soft (maps to image's Triangle).
    #[default]
    Bilinear,
    /// Smoother and sharper than bilinear (CatmullRom).
    Bicubic,
    /// Highest quality, slowest — photos and gradients.
    Lanczos3,
}

impl BackgroundResizeFilter {
    pub fn variant_name(self) -> &'static str {
        match self {
            BackgroundResizeFilter::Nearest => "Nearest",
            BackgroundResizeFilter::Bilinear => "Bilinear",
            BackgroundResizeFilter::Bicubic => "Bicubic",
            BackgroundResizeFilter::Lanczos3 => "Lanczos3",
        }
    }
    pub const ALL: [BackgroundResizeFilter; 4] = [
        BackgroundResizeFilter::Nearest,
        BackgroundResizeFilter::Bilinear,
        BackgroundResizeFilter::Bicubic,
        BackgroundResizeFilter::Lanczos3,
    ];
}

impl BackgroundLayerSpec {
    /// Short label for the authoring UI.
    pub fn label(&self) -> &'static str {
        match self {
            BackgroundLayerSpec::Solid { .. } => "Solid",
            BackgroundLayerSpec::GradientVertical { .. } => "Gradient Vertical",
            BackgroundLayerSpec::GradientHorizontal { .. } => "Gradient Horizontal",
            BackgroundLayerSpec::GradientFourCorner { .. } => "Gradient Four-Corner",
            BackgroundLayerSpec::Starfield { .. } => "Starfield",
            BackgroundLayerSpec::Nebula { .. } => "Nebula",
            BackgroundLayerSpec::Image { .. } => "Image",
        }
    }
}

/// A pre-allocated object pool (`canvas.create_pool(tag, template, count)`).
///
/// Encodes the engine's pooling contract so nobody has to remember it:
/// pooled templates must be manually controlled (gravity 0.0) or parked
/// instances accumulate momentum offscreen and fly on first spawn, and
/// `pool_acquire` resets ONLY position + momentum — rotation/color/scale
/// are the spawner's responsibility.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PoolBlueprint {
    /// Pool tag used by create_pool / pool_acquire / pool_release.
    pub pool_tag: String,
    /// Id of the (spawn_only) object blueprint used as the template.
    pub template_object_id: String,
    /// Number of instances pre-allocated at scene setup.
    pub count: usize,
}

/// A `canvas.add_plugin(...)` registration emitted at the top of setup_scene.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginRegistration {
    /// Plugin type name, e.g. "TerrainCollisionPlugin".
    pub type_name: String,
    /// Constructor expression, e.g. "TerrainCollisionPlugin::new()" or
    /// "SaveGamePlugin::new(\"saves\")". Emitted verbatim inside
    /// canvas.add_plugin(...).
    pub init_expr: String,
}

impl PluginRegistration {
    pub fn new(type_name: impl Into<String>) -> Self {
        let type_name = type_name.into();
        let init_expr = format!("{type_name}::new()");
        Self { type_name, init_expr }
    }

    /// Module import path for the four first-party Quartz plugins; custom
    /// plugins must use a fully-qualified init_expr instead.
    pub fn known_use_path(type_name: &str) -> Option<&'static str> {
        match type_name {
            "TerrainCollisionPlugin" => Some("quartz::plugin::terrain_collision::TerrainCollisionPlugin"),
            "GrapplePlugin" => Some("quartz::plugin::grapple::GrapplePlugin"),
            "BackgroundPlugin" => Some("quartz::plugin::background::BackgroundPlugin"),
            "PathForgePlugin" => Some("quartz_path_forge::PathForgePlugin"),
            "SaveGamePlugin" => Some("quartz::plugin::save_game::SaveGamePlugin"),
            _ => None,
        }
    }

    /// Runtime dispatch name (QuartzPlugin::name()) for the first-party
    /// plugins — what Action::PluginCall/RunPlugin reference.
    pub fn known_dispatch_name(type_name: &str) -> Option<&'static str> {
        match type_name {
            "TerrainCollisionPlugin" => Some("terrain_collision"),
            "GrapplePlugin" => Some("grapple"),
            "BackgroundPlugin" => Some("background"),
            "PathForgePlugin" => Some("path_forge"),
            "SaveGamePlugin" => Some("save_game"),
            _ => None,
        }
    }

    /// Reverse lookup: dispatch name → plugin type name.
    pub fn type_for_dispatch_name(name: &str) -> Option<&'static str> {
        match name {
            "terrain_collision" => Some("TerrainCollisionPlugin"),
            "grapple" => Some("GrapplePlugin"),
            "background" => Some("BackgroundPlugin"),
            "path_forge" => Some("PathForgePlugin"),
            "save_game" => Some("SaveGamePlugin"),
            _ => None,
        }
    }
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
