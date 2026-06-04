use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CanvasOrientation {
    Landscape,
    Portrait,
}

impl Default for CanvasOrientation {
    fn default() -> Self {
        Self::Landscape
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SceneViewBookmark {
    pub id: String,
    pub name: String,
    pub pan_x: f32,
    pub pan_y: f32,
    pub zoom: f32,
}

impl SceneViewBookmark {
    pub fn home_background_cell() -> Self {
        Self {
            id: "bookmark_home".to_owned(),
            name: "Home Background Cell".to_owned(),
            pan_x: 0.0,
            pan_y: 0.0,
            zoom: 1.0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SceneCanvasSpec {
    pub virtual_width: f32,
    pub virtual_height: f32,
    pub zoom: f32,
    pub pan_x: f32,
    pub pan_y: f32,
    #[serde(default)]
    pub show_grid: bool,
    #[serde(default)]
    pub snap_to_grid: bool,
    #[serde(default)]
    pub show_camera_frame: bool,
    #[serde(default)]
    pub camera_x: f32,
    #[serde(default)]
    pub camera_y: f32,
    #[serde(default)]
    pub camera_width: f32,
    #[serde(default)]
    pub camera_height: f32,
    #[serde(default)]
    pub camera_size_customized: bool,
    #[serde(default)]
    pub show_background_cells: bool,
    #[serde(default)]
    pub orientation: CanvasOrientation,
    #[serde(default)]
    pub snap_background_objects_to_cells: bool,
}

impl Default for SceneCanvasSpec {
    fn default() -> Self {
        Self {
            virtual_width: 3840.0,
            virtual_height: 2160.0,
            zoom: 1.0,
            pan_x: 0.0,
            pan_y: 0.0,
            show_grid: true,
            snap_to_grid: false,
            show_camera_frame: true,
            camera_x: 0.0,
            camera_y: 0.0,
            camera_width: 3840.0,
            camera_height: 2160.0,
            camera_size_customized: false,
            show_background_cells: false,
            orientation: CanvasOrientation::Landscape,
            snap_background_objects_to_cells: false,
        }
    }
}

impl SceneCanvasSpec {
    pub fn screen_to_virtual(&self, sx: f32, sy: f32) -> (f32, f32) {
        let z = self.zoom.max(0.001);
        ((sx / z) + self.pan_x, (sy / z) + self.pan_y)
    }

    pub fn virtual_to_screen(&self, vx: f32, vy: f32) -> (f32, f32) {
        let z = self.zoom.max(0.001);
        ((vx - self.pan_x) * z, (vy - self.pan_y) * z)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuartzObjectBlueprint {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub output_file: String,
    #[serde(default)]
    pub visual_asset_mode: ObjectVisualAssetMode,
    #[serde(default)]
    pub visual_asset_path: String,
    #[serde(default = "default_visual_asset_fps")]
    pub visual_asset_fps: f32,
    pub template: ObjectTemplate,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub tags: Vec<String>,
    pub layer: i32,
    pub color_rgb: [u8; 3],
    pub enabled: bool,
    #[serde(default)]
    pub lock_transform: bool,
    #[serde(default)]
    pub is_background: bool,
    #[serde(default)]
    pub spawn_only: bool,
    pub advanced: ObjectAdvancedParams,
    pub visible: ObjectParamVisibility,
}

impl QuartzObjectBlueprint {
    pub fn new(id: String, name: String) -> Self {
        Self {
            id,
            name,
            output_file: String::new(),
            visual_asset_mode: ObjectVisualAssetMode::None,
            visual_asset_path: String::new(),
            visual_asset_fps: default_visual_asset_fps(),
            template: ObjectTemplate::Rectangle,
            x: 400.0,
            y: 300.0,
            w: 120.0,
            h: 120.0,
            tags: Vec::new(),
            layer: 0,
            color_rgb: [255, 255, 255],
            enabled: true,
            lock_transform: false,
            is_background: false,
            spawn_only: false,
            advanced: ObjectAdvancedParams::default(),
            visible: ObjectParamVisibility::default(),
        }
    }

    pub fn apply_background_defaults(&mut self, cell_w: f32, cell_h: f32) {
        self.is_background = true;
        self.spawn_only = false;
        self.w = cell_w.max(1.0);
        self.h = cell_h.max(1.0);
        self.layer = -10;
        self.advanced.gravity = 0.0;
        self.advanced.momentum_x = 0.0;
        self.advanced.momentum_y = 0.0;
    }

    pub fn apply_spawn_only_defaults(&mut self) {
        self.spawn_only = true;
        self.is_background = false;
    }
}

fn default_visual_asset_fps() -> f32 {
    12.0
}

fn default_settext_font_asset_path() -> String {
    String::new()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ObjectVisualAssetMode {
    None,
    StaticImage,
    AnimatedSprite,
}

impl Default for ObjectVisualAssetMode {
    fn default() -> Self {
        Self::None
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ObjectTemplate {
    Rectangle,
    Circle,
}

impl ObjectTemplate {
    pub fn as_str(self) -> &'static str {
        match self {
            ObjectTemplate::Rectangle => "Rectangle",
            ObjectTemplate::Circle => "Circle",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ObjectAdvancedParams {
    pub momentum_x: f32,
    pub momentum_y: f32,
    pub resistance_x: f32,
    pub resistance_y: f32,
    pub gravity: f32,
    pub rotation_deg: f32,
    #[serde(default = "default_pivot_component")]
    pub pivot_x: f32,
    #[serde(default = "default_pivot_component")]
    pub pivot_y: f32,
    #[serde(default)]
    pub material: ObjectPhysicsMaterialSpec,
    pub collision_layer: u32,
    pub collision_mask: u32,
    #[serde(default)]
    pub slope_enabled: bool,
    #[serde(default)]
    pub slope_left_offset: f32,
    #[serde(default)]
    pub slope_right_offset: f32,
    #[serde(default)]
    pub slope_auto_rotation: bool,
    #[serde(default)]
    pub one_way: bool,
    #[serde(default)]
    pub surface_velocity_enabled: bool,
    #[serde(default)]
    pub surface_velocity_x: f32,
    #[serde(default)]
    pub surface_normal_enabled: bool,
    #[serde(default = "default_surface_normal_x")]
    pub surface_normal_x: f32,
    #[serde(default = "default_surface_normal_y")]
    pub surface_normal_y: f32,
    #[serde(default)]
    pub align_to_slope: bool,
    #[serde(default = "default_align_to_slope_speed")]
    pub align_to_slope_speed: f32,
    #[serde(default)]
    pub planet_enabled: bool,
    #[serde(default)]
    pub planet_radius: f32,
    #[serde(default)]
    pub gravity_target_enabled: bool,
    #[serde(default)]
    pub gravity_target_tag: String,
    #[serde(default = "default_gravity_strength")]
    pub gravity_strength: f32,
    #[serde(default = "default_gravity_influence_mult")]
    pub gravity_influence_mult: f32,
    #[serde(default)]
    pub gravity_falloff: QuartzGravityFalloff,
    #[serde(default)]
    pub gravity_all_sources: bool,
    #[serde(default)]
    pub gravity_identity_enabled: bool,
    #[serde(default)]
    pub gravity_identity: String,
    #[serde(default)]
    pub auto_align: bool,
    #[serde(default = "default_auto_align_speed")]
    pub auto_align_speed: f32,
    pub ignore_zoom: bool,
    pub screen_space: bool,
}

impl Default for ObjectAdvancedParams {
    fn default() -> Self {
        Self {
            momentum_x: 0.0,
            momentum_y: 0.0,
            resistance_x: 0.0,
            resistance_y: 0.0,
            gravity: 0.0,
            rotation_deg: 0.0,
            pivot_x: 0.5,
            pivot_y: 0.5,
            material: ObjectPhysicsMaterialSpec::default(),
            collision_layer: 1,
            collision_mask: 1,
            slope_enabled: false,
            slope_left_offset: 0.0,
            slope_right_offset: 0.0,
            slope_auto_rotation: false,
            one_way: false,
            surface_velocity_enabled: false,
            surface_velocity_x: 0.0,
            surface_normal_enabled: false,
            surface_normal_x: default_surface_normal_x(),
            surface_normal_y: default_surface_normal_y(),
            align_to_slope: false,
            align_to_slope_speed: default_align_to_slope_speed(),
            planet_enabled: false,
            planet_radius: 0.0,
            gravity_target_enabled: false,
            gravity_target_tag: String::new(),
            gravity_strength: default_gravity_strength(),
            gravity_influence_mult: default_gravity_influence_mult(),
            gravity_falloff: QuartzGravityFalloff::default(),
            gravity_all_sources: false,
            gravity_identity_enabled: false,
            gravity_identity: String::new(),
            auto_align: false,
            auto_align_speed: default_auto_align_speed(),
            ignore_zoom: false,
            screen_space: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum QuartzGravityFalloff {
    Constant,
    Linear,
    InverseSquare,
}

impl Default for QuartzGravityFalloff {
    fn default() -> Self {
        Self::Linear
    }
}

impl QuartzGravityFalloff {
    pub fn as_str(self) -> &'static str {
        match self {
            QuartzGravityFalloff::Constant => "Constant",
            QuartzGravityFalloff::Linear => "Linear",
            QuartzGravityFalloff::InverseSquare => "InverseSquare",
        }
    }
}

impl ObjectAdvancedParams {
    pub fn is_camera_space_pinned(&self) -> bool {
        self.screen_space
    }

    pub fn set_camera_space_pinned(&mut self, enabled: bool) {
        self.screen_space = enabled;
        if enabled {
            // Camera-pinned objects must ignore zoom so they stay in screen space.
            self.ignore_zoom = true;
        }
    }
}

fn default_pivot_component() -> f32 {
    0.5
}

fn default_surface_normal_x() -> f32 {
    0.0
}

fn default_surface_normal_y() -> f32 {
    -1.0
}

fn default_align_to_slope_speed() -> f32 {
    8.0
}

fn default_gravity_strength() -> f32 {
    1.0
}

fn default_gravity_influence_mult() -> f32 {
    3.0
}

fn default_auto_align_speed() -> f32 {
    10.0
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ObjectPhysicsMaterialPreset {
    Default,
    Rubber,
    Ice,
    Metal,
    Wood,
    Stone,
    Bouncy,
    Sticky,
    Glass,
    Feather,
    Custom,
}

impl ObjectPhysicsMaterialPreset {
    pub fn as_str(self) -> &'static str {
        match self {
            ObjectPhysicsMaterialPreset::Default => "Default",
            ObjectPhysicsMaterialPreset::Rubber => "Rubber",
            ObjectPhysicsMaterialPreset::Ice => "Ice",
            ObjectPhysicsMaterialPreset::Metal => "Metal",
            ObjectPhysicsMaterialPreset::Wood => "Wood",
            ObjectPhysicsMaterialPreset::Stone => "Stone",
            ObjectPhysicsMaterialPreset::Bouncy => "Bouncy",
            ObjectPhysicsMaterialPreset::Sticky => "Sticky",
            ObjectPhysicsMaterialPreset::Glass => "Glass",
            ObjectPhysicsMaterialPreset::Feather => "Feather",
            ObjectPhysicsMaterialPreset::Custom => "Custom",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ObjectPhysicsMaterialSpec {
    pub preset: ObjectPhysicsMaterialPreset,
    pub elasticity: f32,
    pub friction: f32,
    pub density: f32,
}

impl Default for ObjectPhysicsMaterialSpec {
    fn default() -> Self {
        Self {
            preset: ObjectPhysicsMaterialPreset::Default,
            elasticity: 0.0,
            friction: 0.5,
            density: 1.0,
        }
    }
}

impl ObjectPhysicsMaterialSpec {
    pub fn resolved_defaults(preset: ObjectPhysicsMaterialPreset) -> Self {
        match preset {
            ObjectPhysicsMaterialPreset::Default => Self::default(),
            ObjectPhysicsMaterialPreset::Rubber => Self { preset, elasticity: 0.8, friction: 0.9, density: 1.1 },
            ObjectPhysicsMaterialPreset::Ice => Self { preset, elasticity: 0.1, friction: 0.05, density: 0.9 },
            ObjectPhysicsMaterialPreset::Metal => Self { preset, elasticity: 0.3, friction: 0.4, density: 7.8 },
            ObjectPhysicsMaterialPreset::Wood => Self { preset, elasticity: 0.4, friction: 0.6, density: 0.6 },
            ObjectPhysicsMaterialPreset::Stone => Self { preset, elasticity: 0.2, friction: 0.7, density: 2.4 },
            ObjectPhysicsMaterialPreset::Bouncy => Self { preset, elasticity: 1.0, friction: 0.3, density: 0.5 },
            ObjectPhysicsMaterialPreset::Sticky => Self { preset, elasticity: 0.0, friction: 1.0, density: 1.0 },
            ObjectPhysicsMaterialPreset::Glass => Self { preset, elasticity: 0.5, friction: 0.2, density: 2.5 },
            ObjectPhysicsMaterialPreset::Feather => Self { preset, elasticity: 0.3, friction: 0.1, density: 0.01 },
            ObjectPhysicsMaterialPreset::Custom => Self { preset, ..Self::default() },
        }
    }

    pub fn to_custom_material(&self) -> Option<(f32, f32, f32)> {
        if matches!(self.preset, ObjectPhysicsMaterialPreset::Custom) {
            Some((self.elasticity, self.friction, self.density))
        } else {
            None
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ObjectParamVisibility {
    pub transform: bool,
    pub physics: bool,
    pub collision: bool,
    pub slope: bool,
    pub planetary: bool,
    pub camera_space: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuartzEventBinding {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub output_file: String,
    pub kind: QuartzEventKind,
    pub listener_target: QuartzTargetRef,
    pub action_target: QuartzTargetRef,
    pub action: Option<QuartzAction>,
    pub linked_logic_tree_id: Option<String>,
    pub referenced_object_ids: Vec<String>,
}

impl QuartzEventBinding {
    pub fn new(id: String, name: String, default_target: QuartzTargetRef) -> Self {
        Self {
            id,
            name,
            output_file: String::new(),
            kind: QuartzEventKind::KeyPress {
                key: "Space".to_owned(),
                modifiers: QuartzKeyModifiers::default(),
            },
            listener_target: default_target.clone(),
            action_target: default_target,
            action: Some(QuartzAction::Custom {
                name: "event_action".to_owned(),
            }),
            linked_logic_tree_id: None,
            referenced_object_ids: Vec::new(),
        }
    }

    pub fn refresh_references(&mut self) {
        let mut refs = Vec::<String>::new();
        self.listener_target.collect_object_refs(&mut refs);
        self.action_target.collect_object_refs(&mut refs);
        if let Some(action) = &self.action {
            action.collect_object_refs(&mut refs);
        }
        refs.sort();
        refs.dedup();
        self.referenced_object_ids = refs;
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum QuartzEventKind {
    Collision,
    BoundaryCollision,
    KeyPress {
        key: String,
        modifiers: QuartzKeyModifiers,
    },
    KeyRelease {
        key: String,
        modifiers: QuartzKeyModifiers,
    },
    KeyHold {
        key: String,
        modifiers: QuartzKeyModifiers,
    },
    Tick,
    Custom {
        name: String,
    },
    MousePress {
        button: QuartzMouseButtonFilter,
    },
    MouseRelease {
        button: QuartzMouseButtonFilter,
    },
    MouseEnter,
    MouseLeave,
    MouseOver,
    MouseScroll {
        axis: QuartzScrollAxisFilter,
    },
    MouseMove,
}

impl QuartzEventKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            QuartzEventKind::Collision => "Collision",
            QuartzEventKind::BoundaryCollision => "BoundaryCollision",
            QuartzEventKind::KeyPress { .. } => "KeyPress",
            QuartzEventKind::KeyRelease { .. } => "KeyRelease",
            QuartzEventKind::KeyHold { .. } => "KeyHold",
            QuartzEventKind::Tick => "Tick",
            QuartzEventKind::Custom { .. } => "Custom",
            QuartzEventKind::MousePress { .. } => "MousePress",
            QuartzEventKind::MouseRelease { .. } => "MouseRelease",
            QuartzEventKind::MouseEnter => "MouseEnter",
            QuartzEventKind::MouseLeave => "MouseLeave",
            QuartzEventKind::MouseOver => "MouseOver",
            QuartzEventKind::MouseScroll { .. } => "MouseScroll",
            QuartzEventKind::MouseMove => "MouseMove",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum QuartzMouseButtonFilter {
    Any,
    Left,
    Right,
    Middle,
}

impl QuartzMouseButtonFilter {
    pub fn as_str(self) -> &'static str {
        match self {
            QuartzMouseButtonFilter::Any => "Any",
            QuartzMouseButtonFilter::Left => "Left",
            QuartzMouseButtonFilter::Right => "Right",
            QuartzMouseButtonFilter::Middle => "Middle",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum QuartzScrollAxisFilter {
    Any,
    X,
    Y,
}

impl QuartzScrollAxisFilter {
    pub fn as_str(self) -> &'static str {
        match self {
            QuartzScrollAxisFilter::Any => "Any",
            QuartzScrollAxisFilter::X => "X",
            QuartzScrollAxisFilter::Y => "Y",
        }
    }
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct QuartzKeyModifiers {
    pub shift: bool,
    pub control: bool,
    pub alt: bool,
    pub meta: bool,
}

impl Default for ObjectParamVisibility {
    fn default() -> Self {
        Self {
            transform: true,
            physics: false,
            collision: false,
            slope: false,
            planetary: false,
            camera_space: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogicTree {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub output_file: String,
    pub nodes: Vec<LogicNode>,
    pub referenced_object_ids: Vec<String>,
}

impl LogicTree {
    pub fn new(id: String, name: String) -> Self {
        Self {
            id,
            name,
            output_file: String::new(),
            nodes: Vec::new(),
            referenced_object_ids: Vec::new(),
        }
    }

    pub fn refresh_references(&mut self) {
        let mut refs = Vec::<String>::new();
        for node in &self.nodes {
            node.collect_object_refs(&mut refs);
        }
        refs.sort();
        refs.dedup();
        self.referenced_object_ids = refs;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CustomCodeKind {
    Constants,
    GameStateVars,
    TypedVars,
    CustomEvents,
    UpdateLoops,
    TopLevel,
    ManualFileOverride,
}

impl CustomCodeKind {
    pub fn as_str(self) -> &'static str {
        match self {
            CustomCodeKind::Constants => "Constants",
            CustomCodeKind::GameStateVars => "GameStateVars",
            CustomCodeKind::TypedVars => "TypedVars",
            CustomCodeKind::CustomEvents => "CustomEvents",
            CustomCodeKind::UpdateLoops => "UpdateLoops",
            CustomCodeKind::TopLevel => "TopLevel",
            CustomCodeKind::ManualFileOverride => "ManualFileOverride",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CustomCodeBlock {
    pub id: String,
    pub name: String,
    pub kind: CustomCodeKind,
    #[serde(default)]
    pub output_file: String,
    #[serde(default)]
    pub code: String,
    #[serde(default)]
    pub custom_event_name: String,
}

impl CustomCodeBlock {
    pub fn new(id: String, name: String, kind: CustomCodeKind, output_file: String) -> Self {
        Self {
            id,
            name,
            kind,
            output_file,
            code: String::new(),
            custom_event_name: String::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum LogicNode {
    Action(QuartzAction),
    Branch {
        condition: QuartzCondition,
        then_nodes: Vec<LogicNode>,
        else_nodes: Vec<LogicNode>,
    },
}

impl LogicNode {
    pub fn collect_object_refs(&self, out: &mut Vec<String>) {
        match self {
            LogicNode::Action(action) => action.collect_object_refs(out),
            LogicNode::Branch {
                condition,
                then_nodes,
                else_nodes,
            } => {
                condition.collect_object_refs(out);
                for n in then_nodes {
                    n.collect_object_refs(out);
                }
                for n in else_nodes {
                    n.collect_object_refs(out);
                }
            }
        }
    }

    pub fn short_label(&self) -> String {
        match self {
            LogicNode::Action(a) => format!("Action: {}", a.short_label()),
            LogicNode::Branch { condition, .. } => {
                format!("Branch If {}", condition.short_label())
            }
        }
    }
}

/// Operator for ModVar actions (mirrors quartz MathOp).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum QuartzMathOp {
    Add,
    Sub,
    Mul,
    Div,
}

impl Default for QuartzMathOp {
    fn default() -> Self { QuartzMathOp::Add }
}

/// The type of literal or reference held by a QuartzExpr.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum QuartzExprKind {
    F32,
    I32,
    Bool,
    Str,
    Var,
}

impl Default for QuartzExprKind {
    fn default() -> Self { QuartzExprKind::F32 }
}

/// A simple expression value for SetVar / ModVar fields.
/// Covers literal constants and variable references.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct QuartzExpr {
    pub kind: QuartzExprKind,
    /// Raw text: "3.14" for F32, "score" for Var, "true" for Bool, etc.
    pub raw: String,
}

impl Default for QuartzExpr {
    fn default() -> Self { Self { kind: QuartzExprKind::F32, raw: "0.0".to_owned() } }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum QuartzAction {
    Teleport {
        target: QuartzTargetRef,
        location: QuartzLocationRef,
    },
    ApplyMomentum {
        target: QuartzTargetRef,
        mx: f32,
        my: f32,
    },
    SetMomentum {
        target: QuartzTargetRef,
        mx: f32,
        my: f32,
    },
    SetResistance {
        target: QuartzTargetRef,
        rx: f32,
        ry: f32,
    },
    SetGravity {
        target: QuartzTargetRef,
        value: f32,
    },
    SetRotation {
        target: QuartzTargetRef,
        deg: f32,
    },
    SetPivot {
        target: QuartzTargetRef,
        x: f32,
        y: f32,
    },
    AddRotation {
        target: QuartzTargetRef,
        deg: f32,
    },
    ApplyRotation {
        target: QuartzTargetRef,
        deg: f32,
    },
    SetSize {
        target: QuartzTargetRef,
        w: f32,
        h: f32,
    },
    SetCollisionLayer {
        target: QuartzTargetRef,
        layer: u32,
    },
    SetCameraRelative {
        target: QuartzTargetRef,
        enabled: bool,
    },
    SetRenderLayer {
        target: QuartzTargetRef,
        layer: i32,
    },
    Show {
        target: QuartzTargetRef,
    },
    Hide {
        target: QuartzTargetRef,
    },
    Toggle {
        target: QuartzTargetRef,
    },
    Remove {
        target: QuartzTargetRef,
    },
    AddTag {
        target: QuartzTargetRef,
        tag: String,
    },
    RemoveTag {
        target: QuartzTargetRef,
        tag: String,
    },
    SetAnimation {
        target: QuartzTargetRef,
        animation_asset: String,
        fps: f32,
    },
    PlaySound {
        path: String,
        volume: f32,
        looping: bool,
    },
    SetZoom {
        value: f32,
    },
    SmoothZoom {
        value: f32,
    },
    RunPlugin {
        name: String,
        data: String,
    },
    Expr {
        raw: String,
    },
    Custom {
        name: String,
    },
    CameraFlash {
        duration_s: f32,
        intensity: f32,
    },
    CameraShake {
        intensity: f32,
        duration_s: f32,
    },
    CameraZoomPunch {
        amount: f32,
        duration_s: f32,
    },
    /// Set a named variable to a literal or variable-reference expression.
    SetVar {
        name: String,
        value: QuartzExpr,
    },
    /// Modify (+=, -=, *=, /=) a named variable by an expression.
    ModVar {
        name: String,
        op: QuartzMathOp,
        operand: QuartzExpr,
    },
    /// Spawn a clone of a spawn-only template object at the given location.
    SpawnObject {
        template_id: String,
        location: QuartzLocationRef,
    },
    /// Set text content on a text-drawable game object.
    /// `color_rgb` is [r, g, b] (0-255 each).
    /// `font_asset_path` is optional and project-root relative; blank falls back to `Font::default()`.
    SetText {
        target: QuartzTargetRef,
        content: String,
        font_size: f32,
        color_rgb: [u8; 3],
        #[serde(default = "default_settext_font_asset_path")]
        font_asset_path: String,
    },
    Conditional {
        condition: QuartzCondition,
        if_true: Box<QuartzAction>,
        if_false: Option<Box<QuartzAction>>,
    },
    Multi {
        actions: Vec<QuartzAction>,
    },
}

impl QuartzAction {
    pub fn short_label(&self) -> String {
        match self {
            QuartzAction::Teleport { target, .. } => format!("Teleport {}", target.short_label()),
            QuartzAction::ApplyMomentum { target, mx, my } => {
                format!("ApplyMomentum {} ({mx:.2}, {my:.2})", target.short_label())
            }
            QuartzAction::SetMomentum { target, mx, my } => {
                format!("SetMomentum {} ({mx:.2}, {my:.2})", target.short_label())
            }
            QuartzAction::SetResistance { target, rx, ry } => {
                format!("SetResistance {} ({rx:.2}, {ry:.2})", target.short_label())
            }
            QuartzAction::SetGravity { target, value } => {
                format!("SetGravity {} -> {value:.2}", target.short_label())
            }
            QuartzAction::SetRotation { target, deg } => {
                format!("SetRotation {} -> {deg:.1}", target.short_label())
            }
            QuartzAction::SetPivot { target, x, y } => {
                format!("SetPivot {} ({x:.2}, {y:.2})", target.short_label())
            }
            QuartzAction::AddRotation { target, deg } => {
                format!("AddRotation {} -> {deg:.1}", target.short_label())
            }
            QuartzAction::ApplyRotation { target, deg } => {
                format!("ApplyRotation {} -> {deg:.1}", target.short_label())
            }
            QuartzAction::SetSize { target, w, h } => {
                format!("SetSize {} ({w:.1}x{h:.1})", target.short_label())
            }
            QuartzAction::SetCollisionLayer { target, layer } => {
                format!("SetCollisionLayer {} -> {}", target.short_label(), layer)
            }
            QuartzAction::SetCameraRelative { target, enabled } => {
                format!("SetCameraRelative {} -> {}", target.short_label(), enabled)
            }
            QuartzAction::SetRenderLayer { target, layer } => {
                format!("SetRenderLayer {} -> {}", target.short_label(), layer)
            }
            QuartzAction::Show { target } => format!("Show {}", target.short_label()),
            QuartzAction::Hide { target } => format!("Hide {}", target.short_label()),
            QuartzAction::Toggle { target } => format!("Toggle {}", target.short_label()),
            QuartzAction::Remove { target } => format!("Remove {}", target.short_label()),
            QuartzAction::AddTag { target, tag } => {
                format!("AddTag {} [{}]", target.short_label(), tag)
            }
            QuartzAction::RemoveTag { target, tag } => {
                format!("RemoveTag {} [{}]", target.short_label(), tag)
            }
            QuartzAction::SetAnimation {
                target,
                animation_asset,
                fps,
            } => {
                format!(
                    "SetAnimation {} [{} @ {:.1}fps]",
                    target.short_label(),
                    animation_asset,
                    fps
                )
            }
            QuartzAction::PlaySound {
                path,
                volume,
                looping,
            } => {
                format!("PlaySound [{} @ {:.2} loop={}]", path, volume, looping)
            }
            QuartzAction::SetZoom { value } => format!("SetZoom {value:.2}"),
            QuartzAction::SmoothZoom { value } => format!("SmoothZoom {value:.2}"),
            QuartzAction::RunPlugin { name, .. } => format!("RunPlugin {name}"),
            QuartzAction::Expr { raw } => format!("Expr({raw})"),
            QuartzAction::Custom { name } => format!("Custom {name}"),
            QuartzAction::CameraFlash {
                duration_s,
                intensity,
            } => format!("CameraFlash {duration_s:.2}s @ {intensity:.2}"),
            QuartzAction::CameraShake {
                intensity,
                duration_s,
            } => format!("CameraShake {duration_s:.2}s @ {intensity:.2}"),
            QuartzAction::CameraZoomPunch { amount, duration_s } => {
                format!("CameraZoomPunch {amount:.2} for {duration_s:.2}s")
            }
            QuartzAction::SetVar { name, value } => format!("SetVar {} = {:?} ({})", name, value.raw, format!("{:?}", value.kind)),
            QuartzAction::ModVar { name, op, operand } => format!("ModVar {} {:?}= {:?}", name, op, operand.raw),
            QuartzAction::SpawnObject { template_id, .. } => format!("SpawnObject [{}]", template_id),
            QuartzAction::SetText { target, content, .. } => format!("SetText {} = {:?}", target.short_label(), content),
            QuartzAction::Conditional { condition, .. } => {
                format!("Conditional {}", condition.short_label())
            }
            QuartzAction::Multi { actions } => format!("Multi [{} actions]", actions.len()),
        }
    }

    pub fn collect_object_refs(&self, out: &mut Vec<String>) {
        match self {
            QuartzAction::Teleport { target, .. }
            | QuartzAction::ApplyMomentum { target, .. }
            | QuartzAction::SetMomentum { target, .. }
            | QuartzAction::SetResistance { target, .. }
            | QuartzAction::SetGravity { target, .. }
            | QuartzAction::SetRotation { target, .. }
            | QuartzAction::SetPivot { target, .. }
            | QuartzAction::AddRotation { target, .. }
            | QuartzAction::ApplyRotation { target, .. }
            | QuartzAction::SetSize { target, .. }
            | QuartzAction::SetCollisionLayer { target, .. }
            | QuartzAction::SetCameraRelative { target, .. }
            | QuartzAction::SetRenderLayer { target, .. }
            | QuartzAction::Show { target }
            | QuartzAction::Hide { target }
            | QuartzAction::Toggle { target }
            | QuartzAction::Remove { target }
            | QuartzAction::AddTag { target, .. }
            | QuartzAction::RemoveTag { target, .. }
            | QuartzAction::SetAnimation { target, .. } => target.collect_object_refs(out),
            QuartzAction::SetText { target, .. } => target.collect_object_refs(out),
            QuartzAction::SetVar { .. }
            | QuartzAction::ModVar { .. }
            | QuartzAction::SpawnObject { .. }
            | QuartzAction::PlaySound { .. }
            | QuartzAction::SetZoom { .. }
            | QuartzAction::SmoothZoom { .. }
            | QuartzAction::RunPlugin { .. }
            | QuartzAction::Expr { .. }
            | QuartzAction::Custom { .. }
            | QuartzAction::CameraFlash { .. }
            | QuartzAction::CameraShake { .. }
            | QuartzAction::CameraZoomPunch { .. } => {}
            QuartzAction::Conditional {
                condition,
                if_true,
                if_false,
            } => {
                condition.collect_object_refs(out);
                if_true.collect_object_refs(out);
                if let Some(if_false) = if_false {
                    if_false.collect_object_refs(out);
                }
            }
            QuartzAction::Multi { actions } => {
                for action in actions {
                    action.collect_object_refs(out);
                }
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum QuartzCondition {
    Always,
    KeyHeld {
        key: String,
    },
    KeyNotHeld {
        key: String,
    },
    Collision {
        target: QuartzTargetRef,
    },
    NoCollision {
        target: QuartzTargetRef,
    },
    CollisionWith {
        object_a: String,
        object_b: String,
    },
    VarCompare {
        variable: String,
        op: CompareOp,
        value: f32,
    },
    VarExists {
        variable: String,
    },
    Expr {
        raw: String,
    },
    And {
        left: Box<QuartzCondition>,
        right: Box<QuartzCondition>,
    },
    Or {
        left: Box<QuartzCondition>,
        right: Box<QuartzCondition>,
    },
    Not {
        inner: Box<QuartzCondition>,
    },
    IsVisible {
        target: QuartzTargetRef,
    },
    IsHidden {
        target: QuartzTargetRef,
    },
    IsMoving {
        target: QuartzTargetRef,
    },
    Grounded {
        target: QuartzTargetRef,
    },
    HasTag {
        target: QuartzTargetRef,
        tag: String,
    },
    IsSleeping {
        target: QuartzTargetRef,
    },
    SpeedAbove {
        target: QuartzTargetRef,
        value: f32,
    },
    SpeedBelow {
        target: QuartzTargetRef,
        value: f32,
    },
    CrystallineEnabled,
    Plugin {
        name: String,
        arg: Option<String>,
    },
}

impl QuartzCondition {
    pub fn short_label(&self) -> String {
        match self {
            QuartzCondition::Always => "Always".to_owned(),
            QuartzCondition::KeyHeld { key } => format!("KeyHeld({key})"),
            QuartzCondition::KeyNotHeld { key } => format!("KeyNotHeld({key})"),
            QuartzCondition::Collision { target } => {
                format!("Collision({})", target.short_label())
            }
            QuartzCondition::NoCollision { target } => {
                format!("NoCollision({})", target.short_label())
            }
            QuartzCondition::CollisionWith { object_a, object_b } => {
                format!("Collision({object_a}, {object_b})")
            }
            QuartzCondition::VarCompare {
                variable,
                op,
                value,
            } => format!("{} {} {}", variable, op.as_str(), value),
            QuartzCondition::VarExists { variable } => {
                format!("VarExists({variable})")
            }
            QuartzCondition::Expr { raw } => format!("Expr({raw})"),
            QuartzCondition::And { left, right } => {
                format!("({}) AND ({})", left.short_label(), right.short_label())
            }
            QuartzCondition::Or { left, right } => {
                format!("({}) OR ({})", left.short_label(), right.short_label())
            }
            QuartzCondition::Not { inner } => format!("NOT ({})", inner.short_label()),
            QuartzCondition::IsVisible { target } => {
                format!("IsVisible({})", target.short_label())
            }
            QuartzCondition::IsHidden { target } => format!("IsHidden({})", target.short_label()),
            QuartzCondition::IsMoving { target } => format!("IsMoving({})", target.short_label()),
            QuartzCondition::Grounded { target } => format!("Grounded({})", target.short_label()),
            QuartzCondition::HasTag { target, tag } => {
                format!("HasTag({}, {})", target.short_label(), tag)
            }
            QuartzCondition::IsSleeping { target } => {
                format!("IsSleeping({})", target.short_label())
            }
            QuartzCondition::SpeedAbove { target, value } => {
                format!("SpeedAbove({}, {})", target.short_label(), value)
            }
            QuartzCondition::SpeedBelow { target, value } => {
                format!("SpeedBelow({}, {})", target.short_label(), value)
            }
            QuartzCondition::CrystallineEnabled => "CrystallineEnabled".to_owned(),
            QuartzCondition::Plugin { name, arg } => {
                if let Some(arg) = arg {
                    format!("Plugin({}, {})", name, arg)
                } else {
                    format!("Plugin({})", name)
                }
            }
        }
    }

    pub fn collect_object_refs(&self, out: &mut Vec<String>) {
        match self {
            QuartzCondition::CollisionWith { object_a, object_b } => {
                out.push(object_a.clone());
                out.push(object_b.clone());
            }
            QuartzCondition::And { left, right } | QuartzCondition::Or { left, right } => {
                left.collect_object_refs(out);
                right.collect_object_refs(out);
            }
            QuartzCondition::Not { inner } => inner.collect_object_refs(out),
            QuartzCondition::Collision { target }
            | QuartzCondition::NoCollision { target }
            | QuartzCondition::IsVisible { target }
            | QuartzCondition::IsHidden { target }
            | QuartzCondition::IsMoving { target }
            | QuartzCondition::Grounded { target }
            | QuartzCondition::HasTag { target, .. }
            | QuartzCondition::IsSleeping { target }
            | QuartzCondition::SpeedAbove { target, .. }
            | QuartzCondition::SpeedBelow { target, .. } => target.collect_object_refs(out),
            QuartzCondition::Always
            | QuartzCondition::KeyHeld { .. }
            | QuartzCondition::KeyNotHeld { .. }
            | QuartzCondition::VarCompare { .. }
            | QuartzCondition::VarExists { .. }
            | QuartzCondition::Expr { .. }
            | QuartzCondition::CrystallineEnabled
            | QuartzCondition::Plugin { .. } => {}
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CompareOp {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

impl CompareOp {
    pub fn as_str(self) -> &'static str {
        match self {
            CompareOp::Eq => "==",
            CompareOp::Ne => "!=",
            CompareOp::Lt => "<",
            CompareOp::Le => "<=",
            CompareOp::Gt => ">",
            CompareOp::Ge => ">=",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum QuartzTargetRef {
    Name(String),
    Tag(String),
    Id(String),
}

impl QuartzTargetRef {
    pub fn short_label(&self) -> String {
        match self {
            QuartzTargetRef::Name(s) => format!("name:{s}"),
            QuartzTargetRef::Tag(s) => format!("tag:{s}"),
            QuartzTargetRef::Id(s) => format!("id:{s}"),
        }
    }

    pub fn collect_object_refs(&self, out: &mut Vec<String>) {
        if let QuartzTargetRef::Name(name) | QuartzTargetRef::Id(name) = self {
            out.push(name.clone());
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum QuartzLocationRef {
    At { x: f32, y: f32 },
    AtTarget(QuartzTargetRef),
}
