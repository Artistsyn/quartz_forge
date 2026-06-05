use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{anyhow, Context, Result};

mod editors;
mod condition_editor;
mod logic_events_editor;
mod syntax_highlight;

use eframe::egui::{self, Align2, Color32, Key, Pos2, Rect, RichText, Sense, Slider, Stroke, TextEdit};
use image::AnimationDecoder;

use crate::core::project::{EditorProjectState, SceneDocument, SceneKind};
use crate::core::quartz_domain::{
    CanvasOrientation, CustomCodeKind, ObjectPhysicsMaterialPreset, ObjectTemplate,
    QuartzGravityFalloff,
    ObjectVisualAssetMode, SceneCanvasSpec,
};
use crate::services::codegen;
use crate::services::hot_reload::{HotReloadService, PreviewState};
use crate::services::persistence;
use crate::services::project_import;
use crate::services::project_sync;
use crate::app::syntax_highlight::code_layouter;

#[derive(Debug, Clone, Copy)]
enum CanvasDragMode {
    Move,
    ResizeTopLeft,
    ResizeTopRight,
    ResizeBottomLeft,
    ResizeBottomRight,
    Rotate,
}

#[derive(Debug, Clone, Copy)]
struct CanvasDrag {
    object_index: usize,
    start_vx: f32,
    start_vy: f32,
    start_x: f32,
    start_y: f32,
    start_w: f32,
    start_h: f32,
    start_rotation_deg: f32,
    pivot_world_x: f32,
    pivot_world_y: f32,
    mode: CanvasDragMode,
}

#[derive(Debug, Clone, Copy)]
struct CameraViewDrag {
    object_index: usize,
    start_vx: f32,
    start_vy: f32,
    start_x: f32,
    start_y: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HelperVarType {
    I32,
    F32,
    Bool,
    Str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GrappleBiasOption {
    None,
    Horizontal,
    Vertical,
}

impl GrappleBiasOption {
    fn as_str(self) -> &'static str {
        match self {
            GrappleBiasOption::None => "None",
            GrappleBiasOption::Horizontal => "Horizontal",
            GrappleBiasOption::Vertical => "Vertical",
        }
    }

    fn to_quartz_expr(self) -> &'static str {
        match self {
            GrappleBiasOption::None => "SwingBias::None",
            GrappleBiasOption::Horizontal => "SwingBias::Horizontal",
            GrappleBiasOption::Vertical => "SwingBias::Vertical",
        }
    }
}

impl HelperVarType {
    fn as_str(self) -> &'static str {
        match self {
            HelperVarType::I32 => "i32",
            HelperVarType::F32 => "f32",
            HelperVarType::Bool => "bool",
            HelperVarType::Str => "string",
        }
    }
}

#[derive(Clone)]
enum AssetPreviewTextures {
    Static(egui::TextureHandle),
    Animated(Vec<egui::TextureHandle>),
}

#[derive(Clone)]
struct GrapplePreviewConfig {
    enabled: bool,
    target_object_id: String,
    use_anchor_object: bool,
    anchor_object_id: String,
    anchor_x: f32,
    anchor_y: f32,
    length: f32,
    stiffness: f32,
    damping: f32,
}

pub struct QuartzForgeApp {
    project_root: Option<PathBuf>,
    project_state: EditorProjectState,
    hot_reload: HotReloadService,
    status_line: String,
    project_sync_report: Option<persistence::ProjectSyncReport>,
    show_project_sync_prompt: bool,
    new_scene_name: String,
    new_project_name: String,
    selected_object_index: usize,
    #[allow(dead_code)]
    selected_logic_tree_index: usize,
    selected_event_index: usize,
    quartz_preview: String,
    probe_screen_x: f32,
    probe_screen_y: f32,
    canvas_drag: Option<CanvasDrag>,
    canvas_has_focus: bool,
    grid_size: f32,
    undock_scene_canvas: bool,
    show_camera_view_window: bool,
    show_camera_view_grid: bool,
    show_pivot_points: bool,
    show_object_menu_window: bool,
    show_event_builder_window: bool,
    dock_object_menu_window: bool,
    dock_event_builder_window: bool,
    show_spawn_overlay: bool,
    show_constants_window: bool,
    show_game_state_window: bool,
    show_typed_vars_window: bool,
    show_custom_events_window: bool,
    show_update_loops_window: bool,
    show_background_designer_window: bool,
    show_top_level_window: bool,
    show_file_browser_window: bool,
    show_startup_prompt: bool,
    camera_view_drag: Option<CameraViewDrag>,
    asset_preview_cache: HashMap<String, AssetPreviewTextures>,
    selected_custom_code_id: String,
    file_browser_selected_rel: String,
    file_browser_editor_text: String,
    file_browser_editor_dirty: bool,
    file_browser_confirm_track_manual: bool,
    file_browser_cached_root: Option<PathBuf>,
    file_browser_cached_files: Vec<String>,
    selected_scene_bookmark_id: String,
    new_scene_bookmark_name: String,
    helper_var_name: String,
    helper_var_value: String,
    helper_var_type: HelperVarType,
    background_key: String,
    background_cache_dir: String,
    background_use_disk_cache: bool,
    background_top_rgb: [u8; 3],
    background_bottom_rgb: [u8; 3],
    background_star_density: u32,
    background_star_seed: u64,
    background_vertical_fade: u32,
    show_grapple_wizard_window: bool,
    grapple_viz_enabled: bool,
    grapple_target_object_id: String,
    grapple_use_anchor_object: bool,
    grapple_anchor_object_id: String,
    grapple_anchor_x: f32,
    grapple_anchor_y: f32,
    grapple_length: f32,
    grapple_stiffness: f32,
    grapple_damping: f32,
    grapple_max_swing_speed: f32,
    grapple_auto_shorten: bool,
    grapple_bias: GrappleBiasOption,
    preview_refresh_pending: bool,
    last_preview_refresh_at: Option<Instant>,
}

impl Default for QuartzForgeApp {
    fn default() -> Self {
        Self {
            project_root: None,
            project_state: EditorProjectState::new("untitled_project".to_owned()),
            hot_reload: HotReloadService::default(),
            status_line: "Create or load a Quartz Forge project to begin.".to_owned(),
            project_sync_report: None,
            show_project_sync_prompt: false,
            new_scene_name: "new_scene".to_owned(),
            new_project_name: "my_quartz_game".to_owned(),
            selected_object_index: 0,
            selected_logic_tree_index: 0,
            selected_event_index: 0,
            quartz_preview: String::new(),
            probe_screen_x: 512.0,
            probe_screen_y: 288.0,
            canvas_drag: None,
            canvas_has_focus: false,
            grid_size: 64.0,
            undock_scene_canvas: false,
            show_camera_view_window: true,
            show_camera_view_grid: true,
            show_pivot_points: false,
            show_object_menu_window: true,
            show_event_builder_window: true,
            dock_object_menu_window: false,
            dock_event_builder_window: false,
            show_spawn_overlay: true,
            show_constants_window: false,
            show_game_state_window: false,
            show_typed_vars_window: false,
            show_custom_events_window: false,
            show_update_loops_window: false,
            show_background_designer_window: false,
            show_top_level_window: false,
            show_file_browser_window: false,
            show_startup_prompt: true,
            camera_view_drag: None,
            asset_preview_cache: HashMap::new(),
            selected_custom_code_id: String::new(),
            file_browser_selected_rel: String::new(),
            file_browser_editor_text: String::new(),
            file_browser_editor_dirty: false,
            file_browser_confirm_track_manual: false,
            file_browser_cached_root: None,
            file_browser_cached_files: Vec::new(),
            selected_scene_bookmark_id: "bookmark_home".to_owned(),
            new_scene_bookmark_name: "waypoint".to_owned(),
            helper_var_name: "score".to_owned(),
            helper_var_value: "0".to_owned(),
            helper_var_type: HelperVarType::I32,
            background_key: "sky".to_owned(),
            background_cache_dir: "cache/backgrounds".to_owned(),
            background_use_disk_cache: false,
            background_top_rgb: [8, 26, 74],
            background_bottom_rgb: [104, 194, 255],
            background_star_density: 300,
            background_star_seed: 0xCAFE_BABE,
            background_vertical_fade: 200,
            show_grapple_wizard_window: false,
            grapple_viz_enabled: true,
            grapple_target_object_id: "player".to_owned(),
            grapple_use_anchor_object: false,
            grapple_anchor_object_id: String::new(),
            grapple_anchor_x: 900.0,
            grapple_anchor_y: 300.0,
            grapple_length: 260.0,
            grapple_stiffness: 0.8,
            grapple_damping: 0.05,
            grapple_max_swing_speed: 0.0,
            grapple_auto_shorten: false,
            grapple_bias: GrappleBiasOption::None,
            preview_refresh_pending: false,
            last_preview_refresh_at: None,
        }
    }
}

impl QuartzForgeApp {
    fn top_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            if ui.button("New Project").clicked() {
                self.create_project_interactive();
            }
            if ui.button("Open Project").clicked() {
                self.open_project_interactive();
            }
            if ui.button("Save").clicked() {
                self.save_project();
            }
            if ui.button("Generate Quartz Preview").clicked() {
                self.quartz_preview = self.build_scene_source();
                self.status_line = "Quartz API preview regenerated from active scene.".to_owned();
            }
            if ui.button("Write Generated Script").clicked() {
                self.write_generated_script();
            }

            ui.separator();

            if ui.button("Start Preview").clicked() {
                self.start_preview();
            }
            if ui.button("Stop Preview").clicked() {
                self.stop_preview();
            }

            ui.separator();

            let dirty = if self.project_state.dirty { "*" } else { "" };
            let project_name = &self.project_state.manifest.project_name;
            ui.label(format!("Project: {project_name}{dirty}"));

            let state_text = match self.hot_reload.state {
                PreviewState::Stopped => "Stopped",
                PreviewState::Running => "Running",
                PreviewState::Exited => "Exited",
                PreviewState::Failed => "Failed",
            };
            let state_color = match self.hot_reload.state {
                PreviewState::Stopped => Color32::GRAY,
                PreviewState::Running => Color32::LIGHT_GREEN,
                PreviewState::Exited => Color32::LIGHT_BLUE,
                PreviewState::Failed => Color32::LIGHT_RED,
            };
            ui.label(RichText::new(format!("Preview: {state_text}")).color(state_color));
        });
    }

    fn scenes_panel(&mut self, ui: &mut egui::Ui) {
        ui.heading("Scenes");
        ui.add_space(6.0);

        let scene_rows: Vec<(usize, String)> = self
            .project_state
            .manifest
            .scenes
            .iter()
            .enumerate()
            .map(|(idx, scene)| (idx, format!("{} ({})", scene.name, scene.kind.as_str())))
            .collect();

        for (idx, label) in scene_rows {
            let selected = idx == self.project_state.active_scene_index;
            if ui.selectable_label(selected, label).clicked() {
                self.project_state.set_active_scene(idx);
                let scene_name = self
                    .project_state
                    .manifest
                    .scenes
                    .get(idx)
                    .map(|scene| scene.name.clone())
                    .unwrap_or_else(|| "<unknown>".to_owned());
                self.status_line = format!("Active scene: {scene_name}");
            }
        }

        ui.separator();
        ui.label("Add Scene");
        ui.text_edit_singleline(&mut self.new_scene_name);

        let mut selected_kind = self
            .project_state
            .manifest
            .scenes
            .get(self.project_state.active_scene_index)
            .map(|s| s.kind)
            .unwrap_or(SceneKind::Game);

        egui::ComboBox::from_label("Kind")
            .selected_text(selected_kind.as_str())
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut selected_kind, SceneKind::Game, "Game");
                ui.selectable_value(&mut selected_kind, SceneKind::Ui, "UI");
                ui.selectable_value(&mut selected_kind, SceneKind::Cinematic, "Cinematic");
                ui.selectable_value(&mut selected_kind, SceneKind::Test, "Test");
            });

        if ui.button("+ Add Scene").clicked() {
            let name = self.new_scene_name.trim();
            if !name.is_empty() {
                self.project_state.add_scene(name.to_owned(), selected_kind);
                self.status_line = format!("Added scene: {name}");
            }
        }

        if ui.button("- Remove Active Scene").clicked() {
            let idx = self.project_state.active_scene_index;
            let old_name = self
                .project_state
                .manifest
                .scenes
                .get(idx)
                .map(|s| s.name.clone())
                .unwrap_or_else(|| "<unknown>".to_owned());
            self.project_state.remove_scene(idx);
            self.status_line = format!("Removed scene: {old_name}");
        }
    }

    fn inspector_panel(&mut self, ui: &mut egui::Ui) {
        ui.heading("Inspector");
        ui.add_space(6.0);

        let project_root = self.project_root.clone();

        let Some(scene) = self
            .project_state
            .manifest
            .scenes
            .get_mut(self.project_state.active_scene_index)
        else {
            ui.label("No active scene.");
            return;
        };

        ui.label("Scene Name");
        let name_changed = ui.text_edit_singleline(&mut scene.name).changed();

        ui.label("Scene File (relative)");
        let file_changed = Self::source_file_picker(
            ui,
            "Scene File (relative)",
            project_root.as_deref(),
            &mut self.status_line,
            &mut scene.source_file,
        );

        ui.label("Notes");
        let notes_changed = ui.text_edit_multiline(&mut scene.notes).changed();

        if name_changed || file_changed || notes_changed {
            self.project_state.dirty = true;
        }

        ui.separator();
        ui.label("Canvas Translation Contract");
        let mut canvas_changed = false;
        let virtual_width_changed = ui
            .add(Slider::new(&mut scene.canvas.virtual_width, 320.0..=7680.0).text("virtual width"))
            .changed();
        let virtual_height_changed = ui
            .add(Slider::new(&mut scene.canvas.virtual_height, 180.0..=4320.0).text("virtual height"))
            .changed();
        if virtual_width_changed || virtual_height_changed {
            canvas_changed = true;
            if !scene.canvas.camera_size_customized {
                scene.canvas.camera_width = scene.canvas.virtual_width;
                scene.canvas.camera_height = scene.canvas.virtual_height;
            }
        }
        ui.horizontal(|ui| {
            if ui.button("Landscape Preset (3840x2160)").clicked() {
                scene.canvas.virtual_width = 3840.0;
                scene.canvas.virtual_height = 2160.0;
                scene.canvas.camera_width = scene.canvas.virtual_width;
                scene.canvas.camera_height = scene.canvas.virtual_height;
                scene.canvas.camera_size_customized = false;
                scene.canvas.orientation = CanvasOrientation::Landscape;
                canvas_changed = true;
            }
            if ui.button("Portrait Preset (2160x3840)").clicked() {
                scene.canvas.virtual_width = 2160.0;
                scene.canvas.virtual_height = 3840.0;
                scene.canvas.camera_width = scene.canvas.virtual_width;
                scene.canvas.camera_height = scene.canvas.virtual_height;
                scene.canvas.camera_size_customized = false;
                scene.canvas.orientation = CanvasOrientation::Portrait;
                canvas_changed = true;
            }
            if ui.button("Match Camera To Virtual").clicked() {
                scene.canvas.camera_width = scene.canvas.virtual_width;
                scene.canvas.camera_height = scene.canvas.virtual_height;
                scene.canvas.camera_size_customized = false;
                canvas_changed = true;
            }
        });
        ui.horizontal(|ui| {
            ui.label("orientation");
            canvas_changed |= ui
                .selectable_value(
                    &mut scene.canvas.orientation,
                    CanvasOrientation::Landscape,
                    "landscape",
                )
                .changed();
            canvas_changed |= ui
                .selectable_value(
                    &mut scene.canvas.orientation,
                    CanvasOrientation::Portrait,
                    "portrait",
                )
                .changed();
        });
        canvas_changed |= ui.add(Slider::new(&mut scene.canvas.zoom, 0.1..=5.0).text("zoom")).changed();
        canvas_changed |= ui.add(Slider::new(&mut scene.canvas.pan_x, -5000.0..=5000.0).text("pan x")).changed();
        canvas_changed |= ui.add(Slider::new(&mut scene.canvas.pan_y, -5000.0..=5000.0).text("pan y")).changed();
        canvas_changed |= ui.checkbox(&mut scene.canvas.snap_to_grid, "snap object edits to grid").changed();
        canvas_changed |= ui.checkbox(&mut scene.canvas.show_camera_frame, "show camera frame overlay").changed();
        canvas_changed |= ui.checkbox(&mut scene.canvas.show_grid, "show scene grid").changed();
        canvas_changed |= ui.checkbox(&mut scene.canvas.show_background_cells, "show background cells").changed();
        let bg_snap_toggle_changed = ui
            .checkbox(
                &mut scene.canvas.snap_background_objects_to_cells,
                "snap background objects to nearest background cell",
            )
            .changed();
        canvas_changed |= bg_snap_toggle_changed;
        canvas_changed |= ui.add(Slider::new(&mut scene.canvas.camera_x, -8000.0..=8000.0).text("camera x")).changed();
        canvas_changed |= ui.add(Slider::new(&mut scene.canvas.camera_y, -8000.0..=8000.0).text("camera y")).changed();
        let camera_width_changed = ui
            .add(Slider::new(&mut scene.canvas.camera_width, 64.0..=7680.0).text("camera width"))
            .changed();
        let camera_height_changed = ui
            .add(Slider::new(&mut scene.canvas.camera_height, 64.0..=4320.0).text("camera height"))
            .changed();
        if camera_width_changed || camera_height_changed {
            scene.canvas.camera_size_customized = true;
            canvas_changed = true;
        }
        if bg_snap_toggle_changed && scene.canvas.snap_background_objects_to_cells {
            let canvas_snapshot = scene.canvas.clone();
            for object in &mut scene.objects {
                Self::apply_background_snap_if_needed(object, &canvas_snapshot);
            }
        }
        if canvas_changed {
            self.project_state.dirty = true;
        }
        ui.label("Probe Screen -> Virtual roundtrip");
        ui.add(Slider::new(&mut self.probe_screen_x, -2000.0..=4000.0).text("screen x"));
        ui.add(Slider::new(&mut self.probe_screen_y, -2000.0..=3000.0).text("screen y"));
        let (vx, vy) = scene.canvas.screen_to_virtual(self.probe_screen_x, self.probe_screen_y);
        let (sx2, sy2) = scene.canvas.virtual_to_screen(vx, vy);
        ui.label(format!("virtual: ({vx:.2}, {vy:.2})"));
        ui.label(format!("roundtrip: ({sx2:.2}, {sy2:.2})"));

        ui.separator();
        ui.label("Quartz Forge Contract");
        ui.label("- Scene and logic scripts live in /src/scripts");
        ui.label("- Asset roots live in /assets/*");
        ui.label("- Preview runner targets the project root crate");
    }

    fn center_panel(&mut self, ui: &mut egui::Ui) {
        ui.heading("Quartz Authoring Workspace");
        ui.add_space(8.0);

        ui.horizontal(|ui| {
            ui.checkbox(&mut self.undock_scene_canvas, "undock scene canvas window");
            ui.checkbox(&mut self.show_camera_view_window, "show camera view window");
            ui.checkbox(&mut self.show_pivot_points, "show pivot points");
            ui.checkbox(&mut self.show_spawn_overlay, "show spawn overlay");
        });
        ui.horizontal_wrapped(|ui| {
            if ui.checkbox(&mut self.show_object_menu_window, "object menu window").changed() {
                self.dock_object_menu_window = false;
                if self.show_object_menu_window {
                    Self::set_window_open_state(ui.ctx(), egui::Id::new("object_menu_window"), true);
                }
            }
            if ui.checkbox(&mut self.show_event_builder_window, "event builder window").changed() {
                self.dock_event_builder_window = false;
                if self.show_event_builder_window {
                    Self::set_window_open_state(ui.ctx(), egui::Id::new("event_builder_window"), true);
                }
            }
            ui.checkbox(&mut self.show_constants_window, "constants window");
            ui.checkbox(&mut self.show_game_state_window, "game state vars window");
            ui.checkbox(&mut self.show_typed_vars_window, "typed vars window");
            ui.checkbox(&mut self.show_custom_events_window, "custom events window");
            ui.checkbox(&mut self.show_update_loops_window, "update loops window");
            ui.checkbox(&mut self.show_background_designer_window, "background designer window");
            ui.checkbox(&mut self.show_top_level_window, "top level window");
            ui.checkbox(&mut self.show_file_browser_window, "file browser window");
            ui.checkbox(&mut self.show_grapple_wizard_window, "grapple wizard window");
        });

        if self.undock_scene_canvas {
            ui.label("Scene canvas is undocked to a resizable window.");
        } else {
            self.design_canvas(ui);
        }

        ui.separator();
        ui.label("Object menu and event builder now run as collapsible floating windows.");
        ui.label("Use the toggle boxes above to show or hide those windows.");

        ui.separator();
        ui.label("Generated Quartz Syntax Preview");
        egui::ScrollArea::vertical()
            .id_salt("quartz_preview_scroll")
            .max_height(260.0)
            .show(ui, |ui| {
            ui.add(
                TextEdit::multiline(&mut self.quartz_preview)
                    .desired_rows(18)
                    .hint_text("Press Generate Quartz Preview to inspect exported API syntax."),
            );
            });
    }

    fn floating_windows(&mut self, ctx: &egui::Context) {
        if self.undock_scene_canvas {
            egui::Window::new("Scene Canvas")
                .resizable(true)
                .default_size(egui::vec2(1000.0, 520.0))
                .show(ctx, |ui| {
                    self.design_canvas(ui);
                });
        }

        if self.show_camera_view_window {
            egui::Window::new("Camera View")
                .resizable(true)
                .default_size(egui::vec2(560.0, 380.0))
                .default_height(420.0)
                .show(ctx, |ui| {
                    self.camera_view_panel(ui);
                });
        }

        let object_menu_window_id = egui::Id::new("object_menu_window");
        if self.show_object_menu_window && !self.dock_object_menu_window {
            egui::Window::new("Object Menu")
                .id(object_menu_window_id)
                .resizable(true)
                .collapsible(true)
                .default_size(egui::vec2(440.0, 620.0))
                .show(ctx, |ui| {
                    egui::ScrollArea::vertical()
                        .id_salt("object_menu_window_scroll")
                        .show(ui, |ui| {
                            self.objects_editor(ui);
                        });
                });
            if Self::window_is_collapsed(ctx, object_menu_window_id) {
                self.dock_object_menu_window = true;
            }
        }

        let event_builder_window_id = egui::Id::new("event_builder_window");
        if self.show_event_builder_window && !self.dock_event_builder_window {
            egui::Window::new("Event Builder")
                .id(event_builder_window_id)
                .resizable(true)
                .collapsible(true)
                .default_size(egui::vec2(520.0, 620.0))
                .show(ctx, |ui| {
                    egui::ScrollArea::vertical()
                        .id_salt("event_builder_window_scroll")
                        .show(ui, |ui| {
                            self.events_editor(ui);
                        });
                });
            if Self::window_is_collapsed(ctx, event_builder_window_id) {
                self.dock_event_builder_window = true;
            }
        }

        if self.show_constants_window {
            self.custom_code_window(ctx, CustomCodeKind::Constants, "Constants");
        }
        if self.show_game_state_window {
            self.custom_code_window(ctx, CustomCodeKind::GameStateVars, "GameState Vars");
        }
        if self.show_typed_vars_window {
            self.custom_code_window(ctx, CustomCodeKind::TypedVars, "Typed Vars");
        }
        if self.show_custom_events_window {
            self.custom_code_window(ctx, CustomCodeKind::CustomEvents, "Custom Events");
        }
        if self.show_update_loops_window {
            self.custom_code_window(ctx, CustomCodeKind::UpdateLoops, "Update Loops");
        }
        if self.show_background_designer_window {
            self.background_designer_window(ctx);
        }
        if self.show_top_level_window {
            self.custom_code_window(ctx, CustomCodeKind::TopLevel, "Top Level Code");
        }
        if self.show_file_browser_window {
            self.generated_file_browser_window(ctx);
        }
        if self.show_grapple_wizard_window {
            self.grapple_wizard_window(ctx);
        }

        self.floating_window_dock_tray(ctx);
    }

    fn window_is_collapsed(ctx: &egui::Context, window_id: egui::Id) -> bool {
        !egui::collapsing_header::CollapsingState::load_with_default_open(
            ctx,
            window_id.with("collapsing"),
            true,
        )
        .is_open()
    }

    fn set_window_open_state(ctx: &egui::Context, window_id: egui::Id, open: bool) {
        let mut state = egui::collapsing_header::CollapsingState::load_with_default_open(
            ctx,
            window_id.with("collapsing"),
            true,
        );
        state.set_open(open);
        state.store(ctx);
        ctx.request_repaint();
    }

    fn floating_window_dock_tray(&mut self, ctx: &egui::Context) {
        if !((self.show_object_menu_window && self.dock_object_menu_window)
            || (self.show_event_builder_window && self.dock_event_builder_window))
        {
            return;
        }

        egui::Area::new(egui::Id::new("floating_window_dock_tray"))
            .anchor(Align2::RIGHT_BOTTOM, egui::vec2(-16.0, -16.0))
            .show(ctx, |ui| {
                egui::Frame::window(ui.style())
                    .inner_margin(egui::Margin::same(8.0))
                    .show(ui, |ui| {
                        ui.label("Docked Windows");
                        ui.horizontal_wrapped(|ui| {
                            if self.show_object_menu_window
                                && self.dock_object_menu_window
                                && ui.button("Object Menu").clicked()
                            {
                                self.dock_object_menu_window = false;
                                Self::set_window_open_state(ctx, egui::Id::new("object_menu_window"), true);
                            }
                            if self.show_event_builder_window
                                && self.dock_event_builder_window
                                && ui.button("Event Builder").clicked()
                            {
                                self.dock_event_builder_window = false;
                                Self::set_window_open_state(ctx, egui::Id::new("event_builder_window"), true);
                            }
                        });
                    });
            });
    }

    fn active_scene_has_animated_assets(&self) -> bool {
        self.project_state
            .manifest
            .scenes
            .get(self.project_state.active_scene_index)
            .map(|scene| {
                scene.objects.iter().any(|obj| {
                    obj.enabled
                        && (!obj.spawn_only || self.show_spawn_overlay)
                        && obj.visual_asset_mode == ObjectVisualAssetMode::AnimatedSprite
                        && !obj.visual_asset_path.trim().is_empty()
                })
            })
            .unwrap_or(false)
    }

    fn design_canvas(&mut self, ui: &mut egui::Ui) {
        let project_root = self.project_root.clone();
        if let Some(scene) = self
            .project_state
            .manifest
            .scenes
            .get_mut(self.project_state.active_scene_index)
        {
            Self::ensure_home_view_bookmark(scene);
        }

        let bookmark_rows: Vec<(String, String)> = self
            .project_state
            .manifest
            .scenes
            .get(self.project_state.active_scene_index)
            .map(|scene| {
                scene
                    .view_bookmarks
                    .iter()
                    .map(|b| (b.id.clone(), b.name.clone()))
                    .collect()
            })
            .unwrap_or_default();

        ui.horizontal(|ui| {
            ui.label("Visual Scene Canvas");
            ui.add(Slider::new(&mut self.grid_size, 8.0..=256.0).text("grid"));
            egui::ComboBox::from_label("bookmark")
                .selected_text(
                    bookmark_rows
                        .iter()
                        .find(|(id, _)| *id == self.selected_scene_bookmark_id)
                        .map(|(_, name)| name.as_str())
                        .unwrap_or("<none>"),
                )
                .show_ui(ui, |ui| {
                    for (id, name) in &bookmark_rows {
                        ui.selectable_value(&mut self.selected_scene_bookmark_id, id.clone(), name);
                    }
                });
            if ui.button("Go").clicked() {
                self.jump_to_selected_bookmark();
            }
            if ui.button("Home").clicked() {
                self.selected_scene_bookmark_id = "bookmark_home".to_owned();
                self.jump_to_selected_bookmark();
            }
            ui.label("name");
            ui.text_edit_singleline(&mut self.new_scene_bookmark_name);
            if ui.button("+ Waypoint").clicked() {
                self.add_current_view_bookmark();
            }
            if ui.button("Delete Waypoint").clicked() {
                self.delete_selected_bookmark();
            }
            ui.label("Drag objects to move. Use corner nodes to resize and top node to rotate. Arrow keys nudge (Shift = x10). Click canvas to focus.");
        });

        let size = egui::vec2(ui.available_width(), ui.available_height().max(380.0));
        let (rect, response) = ui.allocate_exact_size(size, Sense::click_and_drag());
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 4.0, Color32::from_rgb(22, 24, 28));

        let (project_state, asset_preview_cache) =
            (&mut self.project_state, &mut self.asset_preview_cache);
        let grapple_preview_cfg = GrapplePreviewConfig {
            enabled: self.grapple_viz_enabled,
            target_object_id: self.grapple_target_object_id.clone(),
            use_anchor_object: self.grapple_use_anchor_object,
            anchor_object_id: self.grapple_anchor_object_id.clone(),
            anchor_x: self.grapple_anchor_x,
            anchor_y: self.grapple_anchor_y,
            length: self.grapple_length,
            stiffness: self.grapple_stiffness,
            damping: self.grapple_damping,
        };

        let Some(scene) = project_state
            .manifest
            .scenes
            .get_mut(project_state.active_scene_index)
        else {
            return;
        };

        let mut changed = false;

        if response.hovered() {
            let (raw_scroll_y, smooth_scroll_y, pointer_pos, pointer_delta, middle_down, right_down) = ui.input(|i| {
                (
                    i.raw_scroll_delta.y,
                    i.smooth_scroll_delta.y,
                    i.pointer.hover_pos(),
                    i.pointer.delta(),
                    i.pointer.middle_down(),
                    i.pointer.secondary_down(),
                )
            });

            if middle_down && (pointer_delta.x.abs() > f32::EPSILON || pointer_delta.y.abs() > f32::EPSILON) {
                let z = scene.canvas.zoom.max(0.001);
                // Middle-mouse pans the camera frame through world space.
                scene.canvas.camera_x += pointer_delta.x / z;
                scene.canvas.camera_y += pointer_delta.y / z;
                changed = true;
            }

            if right_down && (pointer_delta.x.abs() > f32::EPSILON || pointer_delta.y.abs() > f32::EPSILON) {
                let z = scene.canvas.zoom.max(0.001);
                // Right-mouse pans the editor viewport over the world.
                scene.canvas.pan_x -= pointer_delta.x / z;
                scene.canvas.pan_y -= pointer_delta.y / z;
                changed = true;
            }

            let scroll_y = if raw_scroll_y.abs() > f32::EPSILON {
                raw_scroll_y
            } else {
                smooth_scroll_y
            };
            let has_wheel = raw_scroll_y.abs() > f32::EPSILON || smooth_scroll_y.abs() > f32::EPSILON;

            if has_wheel {
                let pivot = pointer_pos.unwrap_or(rect.center());
                let before_z = scene.canvas.zoom.max(0.001);
                let before_x = ((pivot.x - rect.left()) / before_z) + scene.canvas.pan_x;
                let before_y = ((pivot.y - rect.top()) / before_z) + scene.canvas.pan_y;

                if scroll_y.abs() > f32::EPSILON {
                    let factor = (1.0 + scroll_y * 0.0015).clamp(0.25, 4.0);
                    scene.canvas.zoom = (scene.canvas.zoom * factor).clamp(0.1, 5.0);
                }

                let after_z = scene.canvas.zoom.max(0.001);
                let after_x = ((pivot.x - rect.left()) / after_z) + scene.canvas.pan_x;
                let after_y = ((pivot.y - rect.top()) / after_z) + scene.canvas.pan_y;
                scene.canvas.pan_x += before_x - after_x;
                scene.canvas.pan_y += before_y - after_y;
                Self::consume_scroll_wheel(ui);
                changed = true;
            }
        }

        let to_screen = |vx: f32, vy: f32| -> Pos2 {
            let (sx, sy) = scene.canvas.virtual_to_screen(vx, vy);
            Pos2::new(rect.left() + sx, rect.top() + sy)
        };
        let to_virtual = |sx: f32, sy: f32| -> (f32, f32) {
            scene
                .canvas
                .screen_to_virtual(sx - rect.left(), sy - rect.top())
        };

        let mut draw_order: Vec<usize> = (0..scene.objects.len()).collect();
        draw_order.sort_by_key(|&idx| (scene.objects[idx].layer, idx as i32));

        let (cell_w, cell_h) = Self::background_cell_size(&scene.canvas);

        if scene.canvas.show_grid {
            let view_min_vx = scene.canvas.pan_x;
            let view_min_vy = scene.canvas.pan_y;
            let view_max_vx = scene.canvas.pan_x + rect.width() / scene.canvas.zoom.max(0.001);
            let view_max_vy = scene.canvas.pan_y + rect.height() / scene.canvas.zoom.max(0.001);
            let grid_step = self.grid_size.max(1.0);
            let minor = Stroke::new(1.0, Color32::from_gray(46));
            let major = Stroke::new(1.0, Color32::from_gray(74));

            let start_ix = (view_min_vx / grid_step).floor() as i32 - 1;
            let end_ix = (view_max_vx / grid_step).ceil() as i32 + 1;
            for ix in start_ix..=end_ix {
                let vx = ix as f32 * grid_step;
                let sx = to_screen(vx, 0.0).x;
                let stroke = if ix.rem_euclid(4) == 0 { major } else { minor };
                painter.line_segment([Pos2::new(sx, rect.top()), Pos2::new(sx, rect.bottom())], stroke);
            }

            let start_iy = (view_min_vy / grid_step).floor() as i32 - 1;
            let end_iy = (view_max_vy / grid_step).ceil() as i32 + 1;
            for iy in start_iy..=end_iy {
                let vy = iy as f32 * grid_step;
                let sy = to_screen(0.0, vy).y;
                let stroke = if iy.rem_euclid(4) == 0 { major } else { minor };
                painter.line_segment([Pos2::new(rect.left(), sy), Pos2::new(rect.right(), sy)], stroke);
            }
        }

        if scene.canvas.show_camera_frame {
            let c0 = to_screen(scene.canvas.camera_x, scene.canvas.camera_y);
            let c1 = to_screen(
                scene.canvas.camera_x + scene.canvas.camera_width,
                scene.canvas.camera_y + scene.canvas.camera_height,
            );
            let cam_rect = Rect::from_two_pos(c0, c1);
            painter.rect_stroke(
                cam_rect,
                2.0,
                Stroke::new(2.0, Color32::from_rgb(255, 210, 90)),
            );
        }

        if scene.canvas.show_background_cells {
            let view_min_vx = scene.canvas.pan_x;
            let view_min_vy = scene.canvas.pan_y;
            let view_max_vx = scene.canvas.pan_x + rect.width() / scene.canvas.zoom.max(0.001);
            let view_max_vy = scene.canvas.pan_y + rect.height() / scene.canvas.zoom.max(0.001);

            let start_cx = (view_min_vx / cell_w).floor() as i32 - 1;
            let end_cx = (view_max_vx / cell_w).ceil() as i32 + 1;
            let start_cy = (view_min_vy / cell_h).floor() as i32 - 1;
            let end_cy = (view_max_vy / cell_h).ceil() as i32 + 1;
            let cell_stroke = Stroke::new(1.5, Color32::from_rgba_unmultiplied(130, 190, 255, 120));

            for cx in start_cx..=end_cx {
                for cy in start_cy..=end_cy {
                    let vx = cx as f32 * cell_w;
                    let vy = cy as f32 * cell_h;
                    let p0 = to_screen(vx, vy);
                    let p1 = to_screen(vx + cell_w, vy + cell_h);
                    let cell_rect = Rect::from_two_pos(p0, p1);
                    painter.rect_stroke(cell_rect, 0.0, cell_stroke);
                }
            }
        }

        // Draw objects in layer order so visual stacking matches runtime expectations.
        for idx in draw_order.iter().copied() {
            let obj = &scene.objects[idx];
            if !obj.enabled {
                continue;
            }
            if obj.spawn_only && !self.show_spawn_overlay {
                continue;
            }
            let is_spawn_ghost = obj.spawn_only;
            let draw_x = if obj.advanced.is_camera_space_pinned() {
                obj.x + scene.canvas.camera_x
            } else {
                obj.x
            };
            let draw_y = if obj.advanced.is_camera_space_pinned() {
                obj.y + scene.canvas.camera_y
            } else {
                obj.y
            };
            let p0 = to_screen(draw_x, draw_y);
            let p1 = to_screen(draw_x + obj.w, draw_y + obj.h);
            let obj_rect = Rect::from_two_pos(p0, p1);
            if !obj_rect.intersects(rect) {
                continue;
            }
            let asset_quad = Self::rotated_rect_points(
                obj_rect.min,
                obj_rect.width(),
                obj_rect.height(),
                obj.advanced.pivot_x,
                obj.advanced.pivot_y,
                Self::effective_rotation_deg(obj),
            );
            let selected = idx == self.selected_object_index;
            let fill = if is_spawn_ghost {
                Color32::from_rgba_unmultiplied(255, 180, 80, if selected { 72 } else { 42 })
            } else if selected {
                Color32::from_rgba_unmultiplied(77, 160, 255, 80)
            } else if obj.advanced.is_camera_space_pinned() {
                Color32::from_rgba_unmultiplied(120, 210, 255, 60)
            } else {
                Color32::from_rgba_unmultiplied(obj.color_rgb[0], obj.color_rgb[1], obj.color_rgb[2], 55)
            };
            let stroke = if is_spawn_ghost {
                Stroke::new(1.5, Color32::from_rgba_unmultiplied(255, 180, 80, 180))
            } else if selected {
                Stroke::new(2.0, Color32::from_rgb(77, 160, 255))
            } else if obj.lock_transform {
                Stroke::new(1.5, Color32::from_rgb(230, 150, 120))
            } else {
                Stroke::new(1.0, Color32::from_gray(180))
            };
            painter.rect_filled(obj_rect, 2.0, Color32::from_rgba_unmultiplied(255, 255, 255, 16));
            match obj.template {
                ObjectTemplate::Rectangle => {
                    painter.add(egui::Shape::convex_polygon(asset_quad.to_vec(), fill, stroke));
                }
                ObjectTemplate::Circle => {
                    let center = obj_rect.center();
                    let radius = (obj_rect.width().min(obj_rect.height()) * 0.5).max(1.0);
                    painter.circle_filled(center, radius, fill);
                    painter.circle_stroke(center, radius, stroke);
                }
            }
            let asset_tint = if is_spawn_ghost {
                Color32::from_rgba_unmultiplied(255, 220, 180, 100)
            } else {
                Color32::WHITE
            };
            let _ = Self::paint_object_asset(
                &painter,
                ui.ctx(),
                asset_preview_cache,
                project_root.as_deref(),
                obj,
                obj_rect,
                asset_quad,
                asset_tint,
            );
            if is_spawn_ghost {
                painter.text(
                    obj_rect.center_top() + egui::vec2(0.0, 4.0),
                    egui::Align2::CENTER_TOP,
                    "spawn",
                    egui::FontId::monospace(10.0),
                    Color32::from_rgba_unmultiplied(255, 200, 120, 220),
                );
            }
            if self.show_pivot_points {
                let pivot_world_x = draw_x + obj.w * obj.advanced.pivot_x;
                let pivot_world_y = draw_y + obj.h * obj.advanced.pivot_y;
                let pivot_screen = to_screen(pivot_world_x, pivot_world_y);
                let pivot_stroke = Stroke::new(1.5, Color32::from_rgb(255, 120, 120));
                painter.circle_stroke(pivot_screen, 5.0, pivot_stroke);
                painter.line_segment(
                    [
                        Pos2::new(pivot_screen.x - 6.0, pivot_screen.y),
                        Pos2::new(pivot_screen.x + 6.0, pivot_screen.y),
                    ],
                    pivot_stroke,
                );
                painter.line_segment(
                    [
                        Pos2::new(pivot_screen.x, pivot_screen.y - 6.0),
                        Pos2::new(pivot_screen.x, pivot_screen.y + 6.0),
                    ],
                    pivot_stroke,
                );
            }
            if !obj.lock_transform && selected {
                let node_size = egui::vec2(9.0, 9.0);
                for corner in [obj_rect.left_top(), obj_rect.right_top(), obj_rect.left_bottom(), obj_rect.right_bottom()] {
                    let handle = Rect::from_center_size(corner, node_size);
                    painter.rect_filled(handle, 1.5, Color32::from_rgb(250, 230, 120));
                }
                let rotate_handle_center = Pos2::new(obj_rect.center().x, obj_rect.top() - 22.0);
                painter.circle_filled(rotate_handle_center, 5.0, Color32::from_rgb(240, 180, 255));
                painter.line_segment(
                    [Pos2::new(obj_rect.center().x, obj_rect.top()), rotate_handle_center],
                    Stroke::new(1.0, Color32::from_rgb(200, 160, 255)),
                );
            }
        }

        Self::paint_grapple_preview_scene(&painter, rect, scene, &to_screen, &grapple_preview_cfg);

        if response.clicked() {
            self.canvas_has_focus = true;
        }

        if response.hovered() {
            let (dx, dy) = ui.input(|i| {
                let step = if i.modifiers.shift { 10.0 } else { 1.0 };
                let mut dx = 0.0;
                let mut dy = 0.0;
                if self.canvas_has_focus && i.key_pressed(Key::ArrowLeft) {
                    dx -= step;
                }
                if self.canvas_has_focus && i.key_pressed(Key::ArrowRight) {
                    dx += step;
                }
                if self.canvas_has_focus && i.key_pressed(Key::ArrowUp) {
                    dy -= step;
                }
                if self.canvas_has_focus && i.key_pressed(Key::ArrowDown) {
                    dy += step;
                }
                (dx, dy)
            });

            if (dx != 0.0 || dy != 0.0) && self.selected_object_index < scene.objects.len() {
                let obj = &mut scene.objects[self.selected_object_index];
                if !obj.lock_transform {
                    obj.x += dx;
                    obj.y += dy;
                    Self::apply_background_snap_if_needed(obj, &scene.canvas);
                    if scene.canvas.snap_to_grid {
                        obj.x = Self::snap_value(obj.x, self.grid_size);
                        obj.y = Self::snap_value(obj.y, self.grid_size);
                    }
                    changed = true;
                }
            }
        }

        // Begin drag selection/resize/rotate
        if response.hovered() && ui.input(|i| i.pointer.primary_pressed()) {
            if let Some(pointer) = ui.input(|i| i.pointer.interact_pos()) {
                let mut hit_index: Option<usize> = None;
                let mut drag_mode = CanvasDragMode::Move;

                for idx in draw_order.iter().copied().rev() {
                    let obj = &scene.objects[idx];
                    if !obj.enabled {
                        continue;
                    }
                    let draw_x = if obj.advanced.is_camera_space_pinned() {
                        obj.x + scene.canvas.camera_x
                    } else {
                        obj.x
                    };
                    let draw_y = if obj.advanced.is_camera_space_pinned() {
                        obj.y + scene.canvas.camera_y
                    } else {
                        obj.y
                    };
                    let p0 = to_screen(draw_x, draw_y);
                    let p1 = to_screen(draw_x + obj.w, draw_y + obj.h);
                    let obj_rect = Rect::from_two_pos(p0, p1);
                    let top_left = Rect::from_center_size(obj_rect.left_top(), egui::vec2(12.0, 12.0));
                    let top_right = Rect::from_center_size(obj_rect.right_top(), egui::vec2(12.0, 12.0));
                    let bottom_left = Rect::from_center_size(obj_rect.left_bottom(), egui::vec2(12.0, 12.0));
                    let bottom_right = Rect::from_center_size(obj_rect.right_bottom(), egui::vec2(12.0, 12.0));
                    let rotate_handle_center = Pos2::new(obj_rect.center().x, obj_rect.top() - 22.0);
                    let rotate_handle = Rect::from_center_size(rotate_handle_center, egui::vec2(14.0, 14.0));

                    if !obj.lock_transform && rotate_handle.contains(pointer) {
                        hit_index = Some(idx);
                        drag_mode = CanvasDragMode::Rotate;
                        break;
                    }
                    if !obj.lock_transform && top_left.contains(pointer) {
                        hit_index = Some(idx);
                        drag_mode = CanvasDragMode::ResizeTopLeft;
                        break;
                    }
                    if !obj.lock_transform && top_right.contains(pointer) {
                        hit_index = Some(idx);
                        drag_mode = CanvasDragMode::ResizeTopRight;
                        break;
                    }
                    if !obj.lock_transform && bottom_left.contains(pointer) {
                        hit_index = Some(idx);
                        drag_mode = CanvasDragMode::ResizeBottomLeft;
                        break;
                    }
                    if !obj.lock_transform && bottom_right.contains(pointer) {
                        hit_index = Some(idx);
                        drag_mode = CanvasDragMode::ResizeBottomRight;
                        break;
                    }
                    if !obj.lock_transform && obj_rect.contains(pointer) {
                        hit_index = Some(idx);
                        drag_mode = CanvasDragMode::Move;
                        break;
                    }
                }

                if let Some(idx) = hit_index {
                    self.selected_object_index = idx;
                    if let Some(obj) = scene.objects.get(idx) {
                        let (svx, svy) = to_virtual(pointer.x, pointer.y);
                        self.canvas_drag = Some(CanvasDrag {
                            object_index: idx,
                            start_vx: svx,
                            start_vy: svy,
                            start_x: obj.x,
                            start_y: obj.y,
                            start_w: obj.w,
                            start_h: obj.h,
                            start_rotation_deg: obj.advanced.rotation_deg,
                            pivot_world_x: obj.x + obj.w * obj.advanced.pivot_x,
                            pivot_world_y: obj.y + obj.h * obj.advanced.pivot_y,
                            mode: drag_mode,
                        });
                    }
                }
            }
        }

        let was_dragging = self.canvas_drag.is_some();

        // Continue drag
        if let Some(drag) = self.canvas_drag {
            if ui.input(|i| i.pointer.primary_down()) {
                if let Some(pointer) = ui.input(|i| i.pointer.interact_pos()) {
                    let (cvx, cvy) = to_virtual(pointer.x, pointer.y);
                    let dx = cvx - drag.start_vx;
                    let dy = cvy - drag.start_vy;
                    if drag.object_index < scene.objects.len() {
                        let obj = &mut scene.objects[drag.object_index];
                        match drag.mode {
                            CanvasDragMode::Move => {
                                obj.x = drag.start_x + dx;
                                obj.y = drag.start_y + dy;
                            }
                            CanvasDragMode::ResizeBottomRight => {
                                obj.w = (drag.start_w + dx).max(2.0);
                                obj.h = (drag.start_h + dy).max(2.0);
                            }
                            CanvasDragMode::ResizeTopLeft => {
                                obj.x = drag.start_x + dx;
                                obj.y = drag.start_y + dy;
                                obj.w = (drag.start_w - dx).max(2.0);
                                obj.h = (drag.start_h - dy).max(2.0);
                            }
                            CanvasDragMode::ResizeTopRight => {
                                obj.y = drag.start_y + dy;
                                obj.w = (drag.start_w + dx).max(2.0);
                                obj.h = (drag.start_h - dy).max(2.0);
                            }
                            CanvasDragMode::ResizeBottomLeft => {
                                obj.x = drag.start_x + dx;
                                obj.w = (drag.start_w - dx).max(2.0);
                                obj.h = (drag.start_h + dy).max(2.0);
                            }
                            CanvasDragMode::Rotate => {
                                let start_angle = (drag.start_vy - drag.pivot_world_y)
                                    .atan2(drag.start_vx - drag.pivot_world_x);
                                let current_angle = (cvy - drag.pivot_world_y)
                                    .atan2(cvx - drag.pivot_world_x);
                                let delta_deg = (current_angle - start_angle).to_degrees();
                                obj.advanced.rotation_deg = drag.start_rotation_deg + delta_deg;
                            }
                        }

                        if scene.canvas.snap_to_grid {
                            match drag.mode {
                                CanvasDragMode::Move
                                | CanvasDragMode::ResizeTopLeft
                                | CanvasDragMode::ResizeBottomLeft => {
                                    obj.x = Self::snap_value(obj.x, self.grid_size);
                                }
                                _ => {}
                            }
                            match drag.mode {
                                CanvasDragMode::Move
                                | CanvasDragMode::ResizeTopLeft
                                | CanvasDragMode::ResizeTopRight => {
                                    obj.y = Self::snap_value(obj.y, self.grid_size);
                                }
                                _ => {}
                            }
                            match drag.mode {
                                CanvasDragMode::ResizeTopLeft
                                | CanvasDragMode::ResizeTopRight
                                | CanvasDragMode::ResizeBottomLeft
                                | CanvasDragMode::ResizeBottomRight => {
                                    obj.w = Self::snap_value(obj.w, self.grid_size).max(2.0);
                                    obj.h = Self::snap_value(obj.h, self.grid_size).max(2.0);
                                }
                                _ => {}
                            }
                        }

                        Self::apply_background_snap_if_needed(obj, &scene.canvas);
                        changed = true;
                    }
                }
            } else {
                self.canvas_drag = None;
            }
        }

        let drag_ended = was_dragging && self.canvas_drag.is_none();
        if changed {
            self.project_state.dirty = true;
        }
        if drag_ended {
            self.touch_and_refresh_preview();
        }
    }

    fn camera_view_panel(&mut self, ui: &mut egui::Ui) {
        let project_root = self.project_root.clone();
        let grapple_preview_cfg = GrapplePreviewConfig {
            enabled: self.grapple_viz_enabled,
            target_object_id: self.grapple_target_object_id.clone(),
            use_anchor_object: self.grapple_use_anchor_object,
            anchor_object_id: self.grapple_anchor_object_id.clone(),
            anchor_x: self.grapple_anchor_x,
            anchor_y: self.grapple_anchor_y,
            length: self.grapple_length,
            stiffness: self.grapple_stiffness,
            damping: self.grapple_damping,
        };
        let (project_state, asset_preview_cache) =
            (&mut self.project_state, &mut self.asset_preview_cache);
        let Some(scene) = project_state
            .manifest
            .scenes
            .get_mut(project_state.active_scene_index)
        else {
            ui.label("No active scene.");
            return;
        };

        let view_w = scene.canvas.camera_width.max(1.0);
        let view_h = scene.canvas.camera_height.max(1.0);
        let mut changed = false;
        ui.horizontal(|ui| {
            ui.label("Camera View");
            ui.checkbox(&mut self.show_camera_view_grid, "show grid");
            ui.label("Drag objects inside the camera frame to reposition them.");
        });

        let aspect = view_w / view_h;
        let target_h = (ui.available_width() / aspect).clamp(220.0, 460.0);
        let size = egui::vec2(ui.available_width(), target_h);
        let (rect, response) = ui.allocate_exact_size(size, Sense::click_and_drag());
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 4.0, Color32::from_rgb(18, 20, 24));
        if response.hovered() {
            let (raw_scroll_y, smooth_scroll_y) = ui.input(|i| (i.raw_scroll_delta.y, i.smooth_scroll_delta.y));
            if raw_scroll_y.abs() > f32::EPSILON || smooth_scroll_y.abs() > f32::EPSILON {
                Self::consume_scroll_wheel(ui);
            }
        }

        let scale = (rect.width() / view_w).min(rect.height() / view_h).max(0.0001);
        let draw_w = view_w * scale;
        let draw_h = view_h * scale;
        let view_rect = Rect::from_min_size(
            Pos2::new(
                rect.left() + (rect.width() - draw_w) * 0.5,
                rect.top() + (rect.height() - draw_h) * 0.5,
            ),
            egui::vec2(draw_w, draw_h),
        );
        painter.rect_stroke(
            view_rect,
            2.0,
            Stroke::new(1.5, Color32::from_rgb(250, 210, 120)),
        );

        if self.show_camera_view_grid {
            let minor = Stroke::new(1.0, Color32::from_gray(58));
            let major = Stroke::new(1.0, Color32::from_gray(92));
            let mut x = 0.0;
            let mut column = 0usize;
            while x <= view_w {
                let stroke = if column % 4 == 0 { major } else { minor };
                let sx = view_rect.left() + x * scale;
                painter.line_segment([Pos2::new(sx, view_rect.top()), Pos2::new(sx, view_rect.bottom())], stroke);
                x += self.grid_size;
                column += 1;
            }
            let mut y = 0.0;
            let mut row = 0usize;
            while y <= view_h {
                let stroke = if row % 4 == 0 { major } else { minor };
                let sy = view_rect.top() + y * scale;
                painter.line_segment([Pos2::new(view_rect.left(), sy), Pos2::new(view_rect.right(), sy)], stroke);
                y += self.grid_size;
                row += 1;
            }
        }

        let mut draw_order: Vec<usize> = (0..scene.objects.len()).collect();
        draw_order.sort_by_key(|&idx| (scene.objects[idx].layer, idx as i32));

        for idx in draw_order.iter().copied() {
            let obj = &scene.objects[idx];
            if !obj.enabled {
                continue;
            }
            if obj.spawn_only && !self.show_spawn_overlay {
                continue;
            }
            let is_spawn_ghost = obj.spawn_only;
            let rel_x = if obj.advanced.is_camera_space_pinned() { obj.x } else { obj.x - scene.canvas.camera_x };
            let rel_y = if obj.advanced.is_camera_space_pinned() { obj.y } else { obj.y - scene.canvas.camera_y };
            let p0 = Pos2::new(view_rect.left() + rel_x * scale, view_rect.top() + rel_y * scale);
            let p1 = Pos2::new(
                view_rect.left() + (rel_x + obj.w) * scale,
                view_rect.top() + (rel_y + obj.h) * scale,
            );
            let obj_rect = Rect::from_two_pos(p0, p1);
            if !obj_rect.intersects(view_rect) {
                continue;
            }
            let asset_quad = Self::rotated_rect_points(
                obj_rect.min,
                obj_rect.width(),
                obj_rect.height(),
                obj.advanced.pivot_x,
                obj.advanced.pivot_y,
                Self::effective_rotation_deg(obj),
            );
            let fill = if is_spawn_ghost {
                Color32::from_rgba_unmultiplied(255, 180, 80, 54)
            } else if obj.advanced.is_camera_space_pinned() {
                Color32::from_rgba_unmultiplied(100, 220, 255, 85)
            } else {
                Color32::from_rgba_unmultiplied(obj.color_rgb[0], obj.color_rgb[1], obj.color_rgb[2], 85)
            };
            let stroke = if is_spawn_ghost {
                Stroke::new(1.25, Color32::from_rgba_unmultiplied(255, 180, 80, 200))
            } else {
                Stroke::new(1.0, Color32::from_gray(230))
            };
            match obj.template {
                ObjectTemplate::Rectangle => {
                    painter.add(egui::Shape::convex_polygon(asset_quad.to_vec(), fill, stroke));
                }
                ObjectTemplate::Circle => {
                    let center = obj_rect.center();
                    let radius = (obj_rect.width().min(obj_rect.height()) * 0.5).max(1.0);
                    painter.circle_filled(center, radius, fill);
                    painter.circle_stroke(center, radius, stroke);
                }
            }
            let asset_tint = if is_spawn_ghost {
                Color32::from_rgba_unmultiplied(255, 220, 180, 100)
            } else {
                Color32::WHITE
            };
            let _ = Self::paint_object_asset(
                &painter,
                ui.ctx(),
                asset_preview_cache,
                project_root.as_deref(),
                obj,
                obj_rect,
                asset_quad,
                asset_tint,
            );
            if is_spawn_ghost {
                painter.text(
                    obj_rect.center_top() + egui::vec2(0.0, 3.0),
                    egui::Align2::CENTER_TOP,
                    "spawn",
                    egui::FontId::monospace(9.0),
                    Color32::from_rgba_unmultiplied(255, 200, 120, 220),
                );
            }
            if self.show_pivot_points {
                let pivot_rel_x = rel_x + obj.w * obj.advanced.pivot_x;
                let pivot_rel_y = rel_y + obj.h * obj.advanced.pivot_y;
                let pivot_screen = Pos2::new(
                    view_rect.left() + pivot_rel_x * scale,
                    view_rect.top() + pivot_rel_y * scale,
                );
                let pivot_stroke = Stroke::new(1.25, Color32::from_rgb(255, 130, 130));
                painter.circle_stroke(pivot_screen, 4.0, pivot_stroke);
                painter.line_segment(
                    [
                        Pos2::new(pivot_screen.x - 5.0, pivot_screen.y),
                        Pos2::new(pivot_screen.x + 5.0, pivot_screen.y),
                    ],
                    pivot_stroke,
                );
                painter.line_segment(
                    [
                        Pos2::new(pivot_screen.x, pivot_screen.y - 5.0),
                        Pos2::new(pivot_screen.x, pivot_screen.y + 5.0),
                    ],
                    pivot_stroke,
                );
            }
        }

        Self::paint_grapple_preview_camera(&painter, scene, view_rect, scale, &grapple_preview_cfg);

        if response.hovered() && ui.input(|i| i.pointer.primary_pressed()) {
            if let Some(pointer) = ui.input(|i| i.pointer.interact_pos()) {
                let mut hit_index = None;
                for idx in draw_order.iter().copied().rev() {
                    let obj = &scene.objects[idx];
                    if !obj.enabled {
                        continue;
                    }
                    let rel_x = if obj.advanced.is_camera_space_pinned() { obj.x } else { obj.x - scene.canvas.camera_x };
                    let rel_y = if obj.advanced.is_camera_space_pinned() { obj.y } else { obj.y - scene.canvas.camera_y };
                    let p0 = Pos2::new(view_rect.left() + rel_x * scale, view_rect.top() + rel_y * scale);
                    let p1 = Pos2::new(
                        view_rect.left() + (rel_x + obj.w) * scale,
                        view_rect.top() + (rel_y + obj.h) * scale,
                    );
                    let obj_rect = Rect::from_two_pos(p0, p1);
                    if obj_rect.contains(pointer) {
                        hit_index = Some(idx);
                        break;
                    }
                }
                if let Some(idx) = hit_index {
                    self.selected_object_index = idx;
                    let vx = (pointer.x - view_rect.left()) / scale;
                    let vy = (pointer.y - view_rect.top()) / scale;
                    if let Some(obj) = scene.objects.get(idx) {
                        self.camera_view_drag = Some(CameraViewDrag {
                            object_index: idx,
                            start_vx: vx,
                            start_vy: vy,
                            start_x: obj.x,
                            start_y: obj.y,
                        });
                    }
                }
            }
        }

        let was_camera_dragging = self.camera_view_drag.is_some();

        if let Some(drag) = self.camera_view_drag {
            if ui.input(|i| i.pointer.primary_down()) {
                if let Some(pointer) = ui.input(|i| i.pointer.interact_pos()) {
                    let vx = (pointer.x - view_rect.left()) / scale;
                    let vy = (pointer.y - view_rect.top()) / scale;
                    let dx = vx - drag.start_vx;
                    let dy = vy - drag.start_vy;
                    if drag.object_index < scene.objects.len() {
                        let obj = &mut scene.objects[drag.object_index];
                        obj.x = drag.start_x + dx;
                        obj.y = drag.start_y + dy;
                        Self::apply_background_snap_if_needed(obj, &scene.canvas);
                        changed = true;
                    }
                }
            } else {
                self.camera_view_drag = None;
            }
        }

        ui.label("Camera view: cyan objects are camera-anchored (screen-space). Others are world-space.");

        let camera_drag_ended = was_camera_dragging && self.camera_view_drag.is_none();
        if changed {
            self.project_state.dirty = true;
        }
        if camera_drag_ended {
            self.touch_and_refresh_preview();
        }
    }

    fn snap_value(value: f32, grid: f32) -> f32 {
        let step = grid.max(1.0);
        (value / step).round() * step
    }

    fn consume_scroll_wheel(ui: &egui::Ui) {
        ui.ctx().input_mut(|i| {
            i.raw_scroll_delta = egui::Vec2::ZERO;
            i.smooth_scroll_delta = egui::Vec2::ZERO;
        });
    }

    fn effective_rotation_deg(object: &crate::core::quartz_domain::QuartzObjectBlueprint) -> f32 {
        if object.advanced.slope_enabled
            && object.advanced.slope_auto_rotation
            && object.w.abs() > f32::EPSILON
        {
            (object.advanced.slope_right_offset - object.advanced.slope_left_offset)
                .atan2(object.w)
                .to_degrees()
        } else {
            object.advanced.rotation_deg
        }
    }

    fn rotated_rect_points(
        top_left: Pos2,
        width: f32,
        height: f32,
        pivot_x: f32,
        pivot_y: f32,
        rotation_deg: f32,
    ) -> [Pos2; 4] {
        let pivot = Pos2::new(top_left.x + width * pivot_x, top_left.y + height * pivot_y);
        let angle = rotation_deg.to_radians();
        let cos_a = angle.cos();
        let sin_a = angle.sin();
        let corners = [
            Pos2::new(top_left.x, top_left.y),
            Pos2::new(top_left.x + width, top_left.y),
            Pos2::new(top_left.x + width, top_left.y + height),
            Pos2::new(top_left.x, top_left.y + height),
        ];

        let mut rotated = [Pos2::ZERO; 4];
        for (idx, corner) in corners.iter().enumerate() {
            let dx = corner.x - pivot.x;
            let dy = corner.y - pivot.y;
            rotated[idx] = Pos2::new(
                pivot.x + (dx * cos_a - dy * sin_a),
                pivot.y + (dx * sin_a + dy * cos_a),
            );
        }
        rotated
    }

    fn ensure_home_view_bookmark(scene: &mut SceneDocument) {
        if !scene.view_bookmarks.iter().any(|b| b.id == "bookmark_home") {
            scene
                .view_bookmarks
                .insert(0, crate::core::quartz_domain::SceneViewBookmark::home_background_cell());
        }
    }

    fn jump_to_selected_bookmark(&mut self) {
        let Some(scene) = self
            .project_state
            .manifest
            .scenes
            .get_mut(self.project_state.active_scene_index)
        else {
            return;
        };
        Self::ensure_home_view_bookmark(scene);
        if let Some(bookmark) = scene
            .view_bookmarks
            .iter()
            .find(|b| b.id == self.selected_scene_bookmark_id)
            .cloned()
        {
            scene.canvas.pan_x = bookmark.pan_x;
            scene.canvas.pan_y = bookmark.pan_y;
            scene.canvas.zoom = bookmark.zoom.clamp(0.1, 5.0);
            self.project_state.dirty = true;
            self.status_line = format!("Jumped to bookmark: {}", bookmark.name);
        }
    }

    fn add_current_view_bookmark(&mut self) {
        let Some(scene) = self
            .project_state
            .manifest
            .scenes
            .get(self.project_state.active_scene_index)
        else {
            return;
        };
        let default_name = format!("waypoint_{}", scene.view_bookmarks.len());
        let name = if self.new_scene_bookmark_name.trim().is_empty() {
            default_name
        } else {
            self.new_scene_bookmark_name.trim().to_owned()
        };
        let pan_x = scene.canvas.pan_x;
        let pan_y = scene.canvas.pan_y;
        let zoom = scene.canvas.zoom;

        self.project_state
            .add_view_bookmark_to_active_scene(name.clone(), pan_x, pan_y, zoom);
        if let Some(scene_mut) = self
            .project_state
            .manifest
            .scenes
            .get(self.project_state.active_scene_index)
        {
            if let Some(last) = scene_mut.view_bookmarks.last() {
                self.selected_scene_bookmark_id = last.id.clone();
            }
        }
        self.status_line = format!("Added bookmark: {name}");
    }

    fn delete_selected_bookmark(&mut self) {
        if self.selected_scene_bookmark_id == "bookmark_home" {
            self.status_line = "Home bookmark cannot be deleted.".to_owned();
            return;
        }
        let selected = self.selected_scene_bookmark_id.clone();
        if let Some(scene) = self
            .project_state
            .manifest
            .scenes
            .get_mut(self.project_state.active_scene_index)
        {
            let before = scene.view_bookmarks.len();
            scene.view_bookmarks.retain(|b| b.id != selected);
            Self::ensure_home_view_bookmark(scene);
            if scene.view_bookmarks.len() != before {
                self.project_state.dirty = true;
                self.selected_scene_bookmark_id = "bookmark_home".to_owned();
                self.status_line = "Deleted selected bookmark.".to_owned();
            }
        }
    }

    fn snap_to_background_cell(value: f32, step: f32) -> f32 {
        let step = step.max(1.0);
        (value / step).round() * step
    }

    fn apply_background_snap_if_needed(
        object: &mut crate::core::quartz_domain::QuartzObjectBlueprint,
        canvas: &SceneCanvasSpec,
    ) {
        if object.is_background && canvas.snap_background_objects_to_cells {
            let (cell_w, cell_h) = Self::background_cell_size(canvas);
            object.x = Self::snap_to_background_cell(object.x, cell_w);
            object.y = Self::snap_to_background_cell(object.y, cell_h);
        }
    }

    fn resolve_anchor_world(
        scene: &crate::core::project::SceneDocument,
        cfg: &GrapplePreviewConfig,
    ) -> Option<(f32, f32)> {
        if cfg.use_anchor_object {
            let anchor = scene
                .objects
                .iter()
                .find(|o| o.id == cfg.anchor_object_id)?;
            Some((anchor.x + anchor.w * 0.5, anchor.y + anchor.h * 0.5))
        } else {
            Some((cfg.anchor_x, cfg.anchor_y))
        }
    }

    fn paint_grapple_preview_scene(
        painter: &egui::Painter,
        rect: Rect,
        scene: &crate::core::project::SceneDocument,
        to_screen: &dyn Fn(f32, f32) -> Pos2,
        cfg: &GrapplePreviewConfig,
    ) {
        if !cfg.enabled {
            return;
        }
        let Some(target) = scene
            .objects
            .iter()
            .find(|o| o.id == cfg.target_object_id)
        else {
            return;
        };
        let Some((anchor_x, anchor_y)) = Self::resolve_anchor_world(scene, cfg) else {
            return;
        };

        let target_center = if target.advanced.is_camera_space_pinned() {
            (
                target.x + scene.canvas.camera_x + target.w * 0.5,
                target.y + scene.canvas.camera_y + target.h * 0.5,
            )
        } else {
            (target.x + target.w * 0.5, target.y + target.h * 0.5)
        };
        let anchor_screen = to_screen(anchor_x, anchor_y);
        let target_screen = to_screen(target_center.0, target_center.1);
        let radius_screen = cfg.length * scene.canvas.zoom.max(0.001);

        painter.circle_stroke(
            anchor_screen,
            radius_screen.max(4.0),
            Stroke::new(1.0, Color32::from_rgba_unmultiplied(180, 230, 255, 80)),
        );
        painter.line_segment(
            [anchor_screen, target_screen],
            Stroke::new(2.0, Color32::from_rgb(110, 220, 255)),
        );
        painter.circle_filled(anchor_screen, 4.0, Color32::from_rgb(255, 220, 110));
        painter.circle_stroke(target_screen, 5.0, Stroke::new(1.5, Color32::from_rgb(110, 220, 255)));
        painter.text(
            Pos2::new(rect.left() + 8.0, rect.top() + 8.0),
            Align2::LEFT_TOP,
            format!(
                "Grapple: {} | L {:.1} | k {:.2} | d {:.2}",
                cfg.target_object_id,
                cfg.length,
                cfg.stiffness,
                cfg.damping
            ),
            egui::FontId::monospace(11.0),
            Color32::from_rgb(170, 230, 255),
        );
    }

    fn paint_grapple_preview_camera(
        painter: &egui::Painter,
        scene: &crate::core::project::SceneDocument,
        view_rect: Rect,
        scale: f32,
        cfg: &GrapplePreviewConfig,
    ) {
        if !cfg.enabled {
            return;
        }
        let Some(target) = scene
            .objects
            .iter()
            .find(|o| o.id == cfg.target_object_id)
        else {
            return;
        };
        let Some((anchor_x, anchor_y)) = Self::resolve_anchor_world(scene, cfg) else {
            return;
        };

        let target_world = if target.advanced.is_camera_space_pinned() {
            (target.x + target.w * 0.5, target.y + target.h * 0.5)
        } else {
            (
                target.x + target.w * 0.5 - scene.canvas.camera_x,
                target.y + target.h * 0.5 - scene.canvas.camera_y,
            )
        };
        let anchor_world = (anchor_x - scene.canvas.camera_x, anchor_y - scene.canvas.camera_y);
        let anchor_screen = Pos2::new(
            view_rect.left() + anchor_world.0 * scale,
            view_rect.top() + anchor_world.1 * scale,
        );
        let target_screen = Pos2::new(
            view_rect.left() + target_world.0 * scale,
            view_rect.top() + target_world.1 * scale,
        );
        painter.line_segment(
            [anchor_screen, target_screen],
            Stroke::new(2.0, Color32::from_rgb(110, 220, 255)),
        );
        painter.circle_filled(anchor_screen, 3.0, Color32::from_rgb(255, 220, 110));
        painter.circle_stroke(target_screen, 4.0, Stroke::new(1.25, Color32::from_rgb(110, 220, 255)));
    }

    fn build_grapple_attach_snippet(&self) -> String {
        let mut chain = String::new();
        if self.grapple_use_anchor_object && !self.grapple_anchor_object_id.trim().is_empty() {
            chain.push_str(&format!(
                "GrappleConstraint::to_object(\"{}\", {})",
                self.grapple_anchor_object_id.trim(),
                self.grapple_length
            ));
        } else {
            chain.push_str(&format!(
                "GrappleConstraint::at_point(({}, {}), {})",
                self.grapple_anchor_x,
                self.grapple_anchor_y,
                self.grapple_length
            ));
        }
        chain.push_str(&format!(".with_stiffness({})", self.grapple_stiffness));
        chain.push_str(&format!(".with_damping({})", self.grapple_damping));
        if self.grapple_max_swing_speed > 0.0 {
            chain.push_str(&format!(".with_max_swing_speed({})", self.grapple_max_swing_speed));
        }
        if self.grapple_auto_shorten {
            chain.push_str(".with_auto_shorten()");
        }
        if self.grapple_bias != GrappleBiasOption::None {
            chain.push_str(&format!(".with_swing_bias({})", self.grapple_bias.to_quartz_expr()));
        }

        format!(
            "canvas.run(Action::PluginCall {{\n    name: \"grapple\".to_owned(),\n    payload: std::sync::Arc::new(GrappleCommand::Attach {{\n        target: Target::name(\"{}\"),\n        grapple: {},\n    }}),\n}});",
            self.grapple_target_object_id.trim(),
            chain
        )
    }

    fn insert_snippet_into_best_custom_block(&mut self, snippet: &str) {
        let Some(scene) = self
            .project_state
            .manifest
            .scenes
            .get_mut(self.project_state.active_scene_index)
        else {
            return;
        };

        let mut target_idx = scene
            .custom_code_blocks
            .iter()
            .position(|b| b.id == self.selected_custom_code_id);
        if target_idx.is_none() {
            target_idx = scene
                .custom_code_blocks
                .iter()
                .position(|b| {
                    matches!(
                        b.kind,
                        CustomCodeKind::UpdateLoops
                            | CustomCodeKind::CustomEvents
                            | CustomCodeKind::TopLevel
                    )
                });
        }
        if target_idx.is_none() {
            scene
                .custom_code_blocks
                .push(crate::core::quartz_domain::CustomCodeBlock::new(
                    "custom_update_0".to_owned(),
                    "custom_update_0".to_owned(),
                    CustomCodeKind::UpdateLoops,
                    String::new(),
                ));
            target_idx = scene.custom_code_blocks.len().checked_sub(1);
        }

        if let Some(idx) = target_idx {
            let block = &mut scene.custom_code_blocks[idx];
            Self::append_code_block(&mut block.code, snippet);
            self.selected_custom_code_id = block.id.clone();
            self.project_state.dirty = true;
            self.quartz_preview = self.build_scene_source();
        }
    }

    fn plugin_install_hint_comment(plugin_name: &str) -> String {
        format!(
            "// git clone https://github.com/Artistsyn/{}.git from within the quartz/src/plugin directory to install this plugin if you have not already",
            plugin_name
        )
    }

    fn background_designer_snippet(&self) -> String {
        let key = self.background_key.trim();
        let key = if key.is_empty() { "sky" } else { key };
        let [tr, tg, tb] = self.background_top_rgb;
        let [br, bg, bb] = self.background_bottom_rgb;
        let cache_arg = if self.background_use_disk_cache {
            let dir = self
                .background_cache_dir
                .trim()
                .replace('\\', "/")
                .replace('"', "\\\"");
            format!("Some(\"{}\")", if dir.is_empty() { "cache/backgrounds" } else { &dir })
        } else {
            "None".to_owned()
        };

        format!(
            "#[cfg(plugin_background)]\n{{\n    use quartz::plugin::background::{{BackgroundLayer, BackgroundPlugin, LayeredBackground}};\n\n    let mut qf_background = BackgroundPlugin::new(1280, 720);\n    let qf_{key}_bg = LayeredBackground::new()\n        .with_layer(BackgroundLayer::GradientVertical {{ top: ({tr}, {tg}, {tb}), bottom: ({br}, {bg}, {bb}) }})\n        .with_layer(BackgroundLayer::Starfield {{\n            density: {density},\n            seed: {seed},\n            size_range: (0, 1),\n            brightness_range: (100, 255),\n            vertical_fade: Some({fade}),\n            scale: None,\n        }});\n\n    qf_background.set_background(\"{key}\", qf_{key}_bg, {cache_arg});\n    canvas.add_plugin(qf_background);\n    canvas.run(Action::run_plugin(\"background\", \"set:{key}\"));\n}}\n\n#[cfg(not(plugin_background))]\n{{\n    // Background plugin not installed.\n    // Install by cloning Artistsyn/background into quartz/src/plugin/background.\n}}\n",
            key = key,
            tr = tr,
            tg = tg,
            tb = tb,
            br = br,
            bg = bg,
            bb = bb,
            density = self.background_star_density,
            seed = self.background_star_seed,
            fade = self.background_vertical_fade,
            cache_arg = cache_arg,
        )
    }

    fn background_designer_window(&mut self, ctx: &egui::Context) {
        egui::Window::new("Background Designer")
            .resizable(true)
            .default_size(egui::vec2(620.0, 620.0))
            .show(ctx, |ui| {
                ui.label("Quartz background plugin authoring (AI+user roundtrip helper)");
                ui.label("Generates plugin-safe code for LayeredBackground + BackgroundPlugin.");
                ui.separator();

                ui.horizontal(|ui| {
                    ui.label("background key");
                    ui.text_edit_singleline(&mut self.background_key);
                });
                ui.horizontal(|ui| {
                    ui.checkbox(&mut self.background_use_disk_cache, "enable disk cache");
                    if self.background_use_disk_cache {
                        ui.label("cache dir");
                        ui.text_edit_singleline(&mut self.background_cache_dir);
                    }
                });

                ui.separator();
                ui.label("Gradient (top -> bottom)");
                ui.horizontal(|ui| {
                    ui.add(Slider::new(&mut self.background_top_rgb[0], 0..=255).text("top r"));
                    ui.add(Slider::new(&mut self.background_top_rgb[1], 0..=255).text("top g"));
                    ui.add(Slider::new(&mut self.background_top_rgb[2], 0..=255).text("top b"));
                });
                ui.horizontal(|ui| {
                    ui.add(Slider::new(&mut self.background_bottom_rgb[0], 0..=255).text("bottom r"));
                    ui.add(Slider::new(&mut self.background_bottom_rgb[1], 0..=255).text("bottom g"));
                    ui.add(Slider::new(&mut self.background_bottom_rgb[2], 0..=255).text("bottom b"));
                });

                ui.separator();
                ui.label("Starfield layer");
                ui.add(Slider::new(&mut self.background_star_density, 10..=3000).text("density"));
                ui.add(Slider::new(&mut self.background_vertical_fade, 0..=2000).text("vertical fade"));
                ui.horizontal(|ui| {
                    ui.label("seed");
                    ui.add(egui::DragValue::new(&mut self.background_star_seed).speed(1.0));
                });

                ui.separator();
                let mut snippet = self.background_designer_snippet();
                ui.label("Generated snippet");
                ui.add(
                    TextEdit::multiline(&mut snippet)
                        .desired_rows(18)
                        .code_editor(),
                );

                ui.horizontal(|ui| {
                    if ui.button("Insert Into Best Custom Block").clicked() {
                        self.ensure_plugin_imports_guard("background", &[]);
                        self.insert_snippet_into_best_custom_block(&snippet);
                        self.status_line = "Inserted background designer snippet into custom code block.".to_owned();
                    }
                    if ui.button("Open Top Level Window").clicked() {
                        self.show_top_level_window = true;
                    }
                });
            });
    }

    fn ensure_plugin_imports_guard(
        &mut self,
        plugin_name: &str,
        use_lines: &[&str],
    ) {
        let active_scene = self.project_state.active_scene_index;
        let needs_top_level = self
            .project_state
            .manifest
            .scenes
            .get(active_scene)
            .map(|scene| {
                !scene
                    .custom_code_blocks
                    .iter()
                    .any(|b| b.kind == CustomCodeKind::TopLevel)
            })
            .unwrap_or(false);
        if needs_top_level {
            self.project_state
                .add_custom_code_block_to_active_scene(CustomCodeKind::TopLevel);
        }

        let Some(scene) = self
            .project_state
            .manifest
            .scenes
            .get_mut(active_scene)
        else {
            return;
        };

        let Some(top_level) = scene
            .custom_code_blocks
            .iter_mut()
            .find(|b| b.kind == CustomCodeKind::TopLevel)
        else {
            return;
        };

        let install_hint = Self::plugin_install_hint_comment(plugin_name);
        if !top_level.code.contains(&install_hint) {
            if !top_level.code.trim().is_empty() {
                top_level.code.push('\n');
            }
            top_level.code.push_str(&install_hint);
            top_level.code.push('\n');
        }

        for use_line in use_lines {
            if !top_level.code.contains(use_line) {
                top_level.code.push_str(use_line);
                top_level.code.push('\n');
            }
        }

        self.project_state.dirty = true;
    }

    fn quartz_value_expr(var_type: HelperVarType, raw_value: &str) -> String {
        match var_type {
            HelperVarType::I32 => format!("Value::I32({})", raw_value.trim().parse::<i32>().unwrap_or(0)),
            HelperVarType::F32 => format!("Value::F32({})", raw_value.trim().parse::<f32>().unwrap_or(0.0)),
            HelperVarType::Bool => {
                let v = matches!(raw_value.trim().to_ascii_lowercase().as_str(), "1" | "true" | "yes" | "on");
                format!("Value::Bool({v})")
            }
            HelperVarType::Str => format!("Value::Str({:?}.to_owned())", raw_value),
        }
    }

    fn quartz_var_getter_name(var_type: HelperVarType) -> &'static str {
        match var_type {
            HelperVarType::I32 => "get_i32",
            HelperVarType::F32 => "get_f32",
            HelperVarType::Bool => "get_bool",
            HelperVarType::Str => "get_str",
        }
    }

    fn append_code_block(code: &mut String, snippet: &str) {
        if !code.trim().is_empty() && !code.ends_with('\n') {
            code.push('\n');
        }
        if !code.trim().is_empty() {
            code.push('\n');
        }
        code.push_str(snippet);
        if !code.ends_with('\n') {
            code.push('\n');
        }
    }

    fn generated_custom_block_preview(block: &crate::core::quartz_domain::CustomCodeBlock) -> String {
        match block.kind {
            CustomCodeKind::UpdateLoops => {
                let mut out = String::new();
                out.push_str("canvas.on_update(|canvas| {\n");
                Self::append_indented_block(&mut out, &block.code, "    ");
                out.push_str("});\n");
                out
            }
            CustomCodeKind::CustomEvents => {
                let mut out = String::new();
                let event_name = if block.custom_event_name.trim().is_empty() {
                    block.name.as_str()
                } else {
                    block.custom_event_name.trim()
                };
                out.push_str(&format!(
                    "canvas.register_custom_event({:?}.to_owned(), |canvas| {{\n",
                    event_name
                ));
                Self::append_indented_block(&mut out, &block.code, "    ");
                out.push_str("});\n");
                out
            }
            _ => block.code.clone(),
        }
    }

    fn try_extract_body_from_generated_preview(kind: CustomCodeKind, preview: &str) -> String {
        match kind {
            CustomCodeKind::UpdateLoops | CustomCodeKind::CustomEvents => {
                let lines: Vec<&str> = preview.lines().collect();
                if lines.len() < 2 {
                    return preview.to_owned();
                }
                let mut body = String::new();
                for line in lines.iter().skip(1).take(lines.len().saturating_sub(2)) {
                    body.push_str(line.strip_prefix("    ").unwrap_or(line));
                    body.push('\n');
                }
                body
            }
            _ => preview.to_owned(),
        }
    }

    fn source_file_picker(
        ui: &mut egui::Ui,
        label: &str,
        project_root: Option<&Path>,
        status_line: &mut String,
        current_value: &mut String,
    ) -> bool {
        let mut changed = false;
        let mut selected = current_value.clone();

        let file_options = project_root.map(Self::collect_rs_files).unwrap_or_default();

        egui::ComboBox::from_label(label)
            .selected_text(if selected.trim().is_empty() {
                "<unset>"
            } else {
                selected.as_str()
            })
            .show_ui(ui, |ui| {
                if ui.selectable_value(&mut selected, String::new(), "<unset>").changed() {
                    changed = true;
                }
                for option in &file_options {
                    if ui.selectable_value(&mut selected, option.clone(), option).changed() {
                        changed = true;
                    }
                }
            });

        ui.horizontal(|ui| {
            if ui.button("Create New File...").clicked() {
                if let Some(root) = project_root {
                    if let Some(path) = rfd::FileDialog::new()
                        .set_title("Create Quartz source file")
                        .set_directory(root)
                        .add_filter("Rust", &["rs"])
                        .save_file()
                    {
                        if let Ok(rel) = path.strip_prefix(root) {
                            selected = rel.to_string_lossy().replace('\\', "/");
                        } else {
                            selected = path.to_string_lossy().replace('\\', "/");
                        }
                        changed = true;
                    }
                } else {
                    *status_line = "Open or create a project before choosing a source file.".to_owned();
                }
            }
            if ui.button("Refresh List").clicked() {
                changed = true;
            }
        });

        if selected != *current_value {
            *current_value = selected;
            changed = true;
        }

        changed
    }

    fn asset_file_picker(
        ui: &mut egui::Ui,
        label: &str,
        project_root: Option<&Path>,
        status_line: &mut String,
        current_value: &mut String,
    ) -> bool {
        let mut changed = false;
        let mut selected = current_value.clone();
        let file_options = project_root.map(Self::collect_asset_files).unwrap_or_default();

        egui::ComboBox::from_label(label)
            .selected_text(if selected.trim().is_empty() {
                "<unset>"
            } else {
                selected.as_str()
            })
            .show_ui(ui, |ui| {
                if ui.selectable_value(&mut selected, String::new(), "<unset>").changed() {
                    changed = true;
                }
                for option in &file_options {
                    if ui.selectable_value(&mut selected, option.clone(), option).changed() {
                        changed = true;
                    }
                }
            });

        ui.horizontal(|ui| {
            if ui.button("Select Existing File...").clicked() {
                if let Some(root) = project_root {
                    if let Some(path) = rfd::FileDialog::new()
                        .set_title("Select object visual asset")
                        .set_directory(root)
                        .add_filter("Image/GIF", &["png", "jpg", "jpeg", "bmp", "webp", "gif"])
                        .pick_file()
                    {
                        if let Some(imported) = Self::import_asset_into_project(root, &path, status_line) {
                            selected = imported;
                        }
                        changed = true;
                    }
                } else {
                    *status_line = "Open or create a project before choosing an asset file.".to_owned();
                }
            }
            if ui.button("Refresh List").clicked() {
                changed = true;
            }
        });

        if selected != *current_value {
            *current_value = selected;
            changed = true;
        }

        changed
    }

    fn collect_rs_files(root: &Path) -> Vec<String> {
        fn visit(dir: &Path, root: &Path, out: &mut Vec<String>) {
            let Ok(entries) = std::fs::read_dir(dir) else { return; };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    visit(&path, root, out);
                } else if path.extension().and_then(|ext| ext.to_str()) == Some("rs") {
                    if let Ok(rel) = path.strip_prefix(root) {
                        out.push(rel.to_string_lossy().replace('\\', "/"));
                    }
                }
            }
        }

        let mut files = Vec::new();
        visit(root, root, &mut files);
        files.sort();
        files.dedup();
        files
    }

    fn collect_asset_files(root: &Path) -> Vec<String> {
        fn is_asset_extension(ext: &str) -> bool {
            matches!(ext, "png" | "jpg" | "jpeg" | "bmp" | "webp" | "gif")
        }

        fn visit(dir: &Path, root: &Path, out: &mut Vec<String>) {
            let Ok(entries) = std::fs::read_dir(dir) else { return; };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    visit(&path, root, out);
                } else if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                    if is_asset_extension(&ext.to_ascii_lowercase()) {
                        if let Ok(rel) = path.strip_prefix(root) {
                            out.push(rel.to_string_lossy().replace('\\', "/"));
                        }
                    }
                }
            }
        }

        let mut files = Vec::new();
        visit(root, root, &mut files);
        files.sort();
        files.dedup();
        files
    }

    fn paint_object_asset(
        painter: &egui::Painter,
        ctx: &egui::Context,
        cache: &mut HashMap<String, AssetPreviewTextures>,
        project_root: Option<&Path>,
        object: &crate::core::quartz_domain::QuartzObjectBlueprint,
        _rect: Rect,
        quad: [Pos2; 4],
        tint: Color32,
    ) -> bool {
        if object.visual_asset_mode == ObjectVisualAssetMode::None {
            return false;
        }
        let rel = object.visual_asset_path.trim();
        if rel.is_empty() {
            return false;
        }
        let Some(path) = project_root.map(|root| root.join(rel)) else {
            return false;
        };
        let cache_key = path.to_string_lossy().replace('\\', "/");
        if !cache.contains_key(&cache_key) {
            let loaded = if object.visual_asset_mode == ObjectVisualAssetMode::AnimatedSprite
                && path
                    .extension()
                    .and_then(|e| e.to_str())
                    .is_some_and(|e| e.eq_ignore_ascii_case("gif"))
            {
                Self::load_gif_textures(ctx, &path)
            } else {
                Self::load_static_texture(ctx, &path)
            };
            if let Some(preview) = loaded {
                cache.insert(cache_key.clone(), preview);
            } else {
                return false;
            }
        }

        let Some(preview) = cache.get(&cache_key).cloned() else {
            return false;
        };
        let texture_id = match preview {
            AssetPreviewTextures::Static(texture) => texture.id(),
            AssetPreviewTextures::Animated(frames) => {
                if frames.is_empty() {
                    return false;
                }
                let fps = object.visual_asset_fps.max(1.0);
                let t = ctx.input(|i| i.time) as f32;
                let frame = ((t * fps) as usize) % frames.len();
                frames[frame].id()
            }
        };

        let mut mesh = egui::epaint::Mesh::with_texture(texture_id);
        let base = mesh.vertices.len() as u32;
        for (pos, uv) in quad.into_iter().zip([
            Pos2::new(0.0, 0.0),
            Pos2::new(1.0, 0.0),
            Pos2::new(1.0, 1.0),
            Pos2::new(0.0, 1.0),
        ]) {
            mesh.vertices.push(egui::epaint::Vertex { pos, uv, color: tint });
        }
        mesh.indices
            .extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
        painter.add(egui::Shape::mesh(mesh));
        true
    }

    fn load_static_texture(ctx: &egui::Context, path: &Path) -> Option<AssetPreviewTextures> {
        let bytes = std::fs::read(path).ok()?;
        let decoded = image::load_from_memory(&bytes).ok()?;
        let rgba = decoded.to_rgba8();
        let size = [rgba.width() as usize, rgba.height() as usize];
        if size[0] == 0 || size[1] == 0 {
            return None;
        }
        let color_image = egui::ColorImage::from_rgba_unmultiplied(size, rgba.as_raw());
        let name = format!("object_asset_static:{}", path.to_string_lossy().replace('\\', "/"));
        Some(AssetPreviewTextures::Static(
            ctx.load_texture(name, color_image, egui::TextureOptions::LINEAR),
        ))
    }

    fn load_gif_textures(ctx: &egui::Context, path: &Path) -> Option<AssetPreviewTextures> {
        let bytes = std::fs::read(path).ok()?;
        let decoder = image::codecs::gif::GifDecoder::new(std::io::Cursor::new(bytes)).ok()?;
        let frames = decoder.into_frames().collect_frames().ok()?;
        if frames.is_empty() {
            return None;
        }

        let mut textures = Vec::with_capacity(frames.len());
        let base = path.to_string_lossy().replace('\\', "/");
        for (idx, frame) in frames.into_iter().enumerate() {
            let rgba = frame.into_buffer();
            let size = [rgba.width() as usize, rgba.height() as usize];
            if size[0] == 0 || size[1] == 0 {
                continue;
            }
            let color_image = egui::ColorImage::from_rgba_unmultiplied(size, rgba.as_raw());
            let name = format!("object_asset_gif:{}:{}", base, idx);
            textures.push(ctx.load_texture(name, color_image, egui::TextureOptions::LINEAR));
        }
        if textures.is_empty() {
            None
        } else {
            Some(AssetPreviewTextures::Animated(textures))
        }
    }

    fn component_target_path(scene_source_file: &str, configured: &str) -> String {
        let configured = configured.trim();
        if configured.is_empty() {
            scene_source_file.to_owned()
        } else {
            configured.to_owned()
        }
    }

    fn component_module_path_attr(scene_source_file: &str, target_file: &str) -> String {
        let scene_dir = Path::new(scene_source_file)
            .parent()
            .unwrap_or_else(|| Path::new(""));
        let target_path = Path::new(target_file);

        let from_components = scene_dir.components().collect::<Vec<_>>();
        let to_components = target_path.components().collect::<Vec<_>>();

        let mut shared_prefix_len = 0usize;
        while shared_prefix_len < from_components.len()
            && shared_prefix_len < to_components.len()
            && from_components[shared_prefix_len] == to_components[shared_prefix_len]
        {
            shared_prefix_len += 1;
        }

        let mut parts = Vec::new();
        for _ in shared_prefix_len..from_components.len() {
            parts.push("..".to_owned());
        }
        for component in to_components.iter().skip(shared_prefix_len) {
            parts.push(component.as_os_str().to_string_lossy().replace('\\', "/"));
        }

        if parts.is_empty() {
            Path::new(target_file)
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_else(|| target_file.replace('\\', "/"))
        } else {
            parts.join("/")
        }
    }

    fn custom_code_function_name(prefix: &str, id: &str) -> String {
        let mut out = String::from(prefix);
        for ch in id.chars() {
            if ch.is_ascii_alphanumeric() {
                out.push(ch.to_ascii_lowercase());
            } else {
                out.push('_');
            }
        }
        out
    }

    fn append_indented_block(out: &mut String, source: &str, indent: &str) {
        for line in source.lines() {
            if line.trim().is_empty() {
                out.push('\n');
            } else {
                out.push_str(indent);
                out.push_str(line);
                out.push('\n');
            }
        }
    }

    fn build_scene_source(&self) -> String {
        let Some(scene) = self
            .project_state
            .manifest
            .scenes
            .get(self.project_state.active_scene_index)
        else {
            return "// no active scene".to_owned();
        };

        let scene_source_file = scene.source_file.trim();
        let mut external_modules: BTreeMap<String, String> = BTreeMap::new();

        for object in &scene.objects {
            let target = Self::component_target_path(scene_source_file, &object.output_file);
            if target != scene_source_file && !external_modules.contains_key(&target) {
                external_modules.insert(target.clone(), codegen::file_module_name(&target));
            }
        }
        for event in &scene.events {
            let target = Self::component_target_path(scene_source_file, &event.output_file);
            if target != scene_source_file && !external_modules.contains_key(&target) {
                external_modules.insert(target.clone(), codegen::file_module_name(&target));
            }
        }
        for tree in &scene.logic_trees {
            let target = Self::component_target_path(scene_source_file, &tree.output_file);
            if target != scene_source_file && !external_modules.contains_key(&target) {
                external_modules.insert(target.clone(), codegen::file_module_name(&target));
            }
        }
        for block in &scene.custom_code_blocks {
            if block.kind == CustomCodeKind::ManualFileOverride {
                continue;
            }
            let target = Self::component_target_path(scene_source_file, &block.output_file);
            if target != scene_source_file && !block.code.trim().is_empty() && !external_modules.contains_key(&target) {
                external_modules.insert(target.clone(), codegen::file_module_name(&target));
            }
        }

        let mut out = String::new();
        out.push_str("use quartz::prelude::*;\n");
        for (target, module_name) in &external_modules {
            let module_path = Self::component_module_path_attr(scene_source_file, target);
            out.push_str(&format!("#[path = \"{}\"]\nmod {};\n", module_path, module_name));
            out.push_str(&format!("use {}::*;\n", module_name));
        }
        if !external_modules.is_empty() {
            out.push_str("\n");
        }

        for block in &scene.custom_code_blocks {
            if block.code.trim().is_empty() {
                continue;
            }
            let target = Self::component_target_path(scene_source_file, &block.output_file);
            if target != scene_source_file {
                continue;
            }
            if matches!(
                block.kind,
                CustomCodeKind::Constants
                    | CustomCodeKind::TopLevel
            ) {
                out.push_str(&format!("// custom code block: {}\n", block.name));
                out.push_str(&block.code);
                out.push_str("\n\n");
            }
        }

        for object in &scene.objects {
            if !object.enabled || !object.spawn_only {
                continue;
            }
            let target = Self::component_target_path(scene_source_file, &object.output_file);
            if target == scene_source_file {
                out.push_str(&codegen::object_function_source(object));
                out.push('\n');
            }
        }

        out.push_str("pub fn setup_scene(canvas: &mut Canvas) {\n");
        for object in &scene.objects {
            if !object.enabled {
                continue;
            }
            if object.spawn_only {
                continue;
            }
            let target = Self::component_target_path(scene_source_file, &object.output_file);
            if target == scene_source_file {
                out.push_str(&codegen::object_registration_body(object));
            } else {
                out.push_str(&format!("    {}(canvas);\n", codegen::object_function_name(object)));
            }
        }
        for block in &scene.custom_code_blocks {
            if !matches!(block.kind, CustomCodeKind::GameStateVars | CustomCodeKind::TypedVars)
                || block.code.trim().is_empty()
            {
                continue;
            }
            let target = Self::component_target_path(scene_source_file, &block.output_file);
            if target == scene_source_file {
                out.push_str(&format!("    // game var block: {}\n", block.name));
                Self::append_indented_block(&mut out, &block.code, "    ");
            } else {
                out.push_str(&format!(
                    "    {}(canvas);\n",
                    Self::custom_code_function_name("init_vars_", &block.id)
                ));
            }
        }
        out.push_str("}\n\n");

        out.push_str("pub fn register_logic(canvas: &mut Canvas) {\n");
        for tree in &scene.logic_trees {
            let target = Self::component_target_path(scene_source_file, &tree.output_file);
            if target == scene_source_file {
                out.push_str(&format!("    // Update Script: {}\n", tree.name));
                out.push_str("    canvas.on_update(|canvas| {\n");
                out.push_str(&format!("        canvas.run({});\n", codegen::logic_tree_action_expr(tree)));
                out.push_str("    });\n");
            } else {
                out.push_str(&format!("    {}(canvas);\n", codegen::logic_tree_function_name(tree)));
            }
        }
        for block in &scene.custom_code_blocks {
            if block.kind != CustomCodeKind::UpdateLoops || block.code.trim().is_empty() {
                continue;
            }
            let target = Self::component_target_path(scene_source_file, &block.output_file);
            if target == scene_source_file {
                out.push_str(&format!("    // custom update loop: {}\n", block.name));
                out.push_str("    canvas.on_update(|canvas| {\n");
                Self::append_indented_block(&mut out, &block.code, "        ");
                out.push_str("    });\n");
            } else {
                out.push_str(&format!(
                    "    {}(canvas);\n",
                    Self::custom_code_function_name("register_update_", &block.id)
                ));
            }
        }
        out.push_str("}\n\n");

        out.push_str("pub fn register_events(canvas: &mut Canvas) {\n");
        for event in &scene.events {
            let target = Self::component_target_path(scene_source_file, &event.output_file);
            if target == scene_source_file {
                out.push_str(&codegen::event_binding_body(event, &scene.logic_trees));
            } else {
                out.push_str(&format!("    {}(canvas);\n", codegen::event_function_name(event)));
            }
        }
        for block in &scene.custom_code_blocks {
            if block.kind != CustomCodeKind::CustomEvents || block.code.trim().is_empty() {
                continue;
            }
            let target = Self::component_target_path(scene_source_file, &block.output_file);
            if target == scene_source_file {
                let event_name = if block.custom_event_name.trim().is_empty() {
                    block.name.clone()
                } else {
                    block.custom_event_name.trim().to_owned()
                };
                out.push_str(&format!("    // custom event: {}\n", block.name));
                out.push_str(&format!(
                    "    canvas.register_custom_event(\"{}\".to_owned(), |canvas| {{\n",
                    event_name
                ));
                Self::append_indented_block(&mut out, &block.code, "        ");
                out.push_str("    });\n");
            } else {
                out.push_str(&format!(
                    "    {}(canvas);\n",
                    Self::custom_code_function_name("register_event_", &block.id)
                ));
            }
        }
        out.push_str("}\n");

        out
    }

    #[allow(dead_code)]
    fn build_component_module_source(&self, target_file: &str) -> Option<String> {
        let scene = self
            .project_state
            .manifest
            .scenes
            .get(self.project_state.active_scene_index)?;
        let scene_source_file = scene.source_file.trim();
        let mut out = String::new();
        let mut wrote_any = false;

        out.push_str("use quartz::prelude::*;\n\n");
        for object in &scene.objects {
            if !object.enabled {
                continue;
            }
            let object_target = Self::component_target_path(scene_source_file, &object.output_file);
            if object_target == target_file {
                out.push_str(&codegen::object_function_source(object));
                out.push('\n');
                wrote_any = true;
            }
        }
        for event in &scene.events {
            let event_target = Self::component_target_path(scene_source_file, &event.output_file);
            if event_target == target_file {
                out.push_str(&codegen::event_function_source(event, &scene.logic_trees));
                out.push('\n');
                wrote_any = true;
            }
        }
        for tree in &scene.logic_trees {
            let tree_target = Self::component_target_path(scene_source_file, &tree.output_file);
            if tree_target == target_file {
                out.push_str(&codegen::logic_tree_function_source(tree));
                out.push('\n');
                wrote_any = true;
            }
        }

        for block in &scene.custom_code_blocks {
            if block.code.trim().is_empty() || block.kind == CustomCodeKind::ManualFileOverride {
                continue;
            }
            let block_target = Self::component_target_path(scene_source_file, &block.output_file);
            if block_target != target_file {
                continue;
            }
            match block.kind {
                CustomCodeKind::Constants | CustomCodeKind::TopLevel => {
                    out.push_str(&format!("// custom code block: {}\n", block.name));
                    out.push_str(&block.code);
                    out.push_str("\n\n");
                }
                CustomCodeKind::GameStateVars | CustomCodeKind::TypedVars => {
                    out.push_str(&format!(
                        "pub fn {}(canvas: &mut Canvas) {{\n",
                        Self::custom_code_function_name("init_vars_", &block.id)
                    ));
                    Self::append_indented_block(&mut out, &block.code, "    ");
                    out.push_str("}\n\n");
                }
                CustomCodeKind::UpdateLoops => {
                    out.push_str(&format!(
                        "pub fn {}(canvas: &mut Canvas) {{\n",
                        Self::custom_code_function_name("register_update_", &block.id)
                    ));
                    out.push_str("    canvas.on_update(|canvas| {\n");
                    Self::append_indented_block(&mut out, &block.code, "        ");
                    out.push_str("    });\n}\n\n");
                }
                CustomCodeKind::CustomEvents => {
                    let event_name = if block.custom_event_name.trim().is_empty() {
                        block.name.clone()
                    } else {
                        block.custom_event_name.trim().to_owned()
                    };
                    out.push_str(&format!(
                        "pub fn {}(canvas: &mut Canvas) {{\n",
                        Self::custom_code_function_name("register_event_", &block.id)
                    ));
                    out.push_str(&format!(
                        "    canvas.register_custom_event(\"{}\".to_owned(), |canvas| {{\n",
                        event_name
                    ));
                    Self::append_indented_block(&mut out, &block.code, "        ");
                    out.push_str("    });\n}\n\n");
                }
                CustomCodeKind::ManualFileOverride => {}
            }
            wrote_any = true;
        }

        if wrote_any {
            Some(out)
        } else {
            None
        }
    }

    fn custom_code_window(&mut self, ctx: &egui::Context, kind: CustomCodeKind, title: &str) {
        egui::Window::new(title)
            .resizable(true)
            .default_size(egui::vec2(620.0, 420.0))
            .show(ctx, |ui| {
                let project_root = self.project_root.clone();

                let rows: Vec<(String, String)> = self
                    .project_state
                    .manifest
                    .scenes
                    .get(self.project_state.active_scene_index)
                    .map(|scene| {
                        scene
                            .custom_code_blocks
                            .iter()
                            .filter(|b| b.kind == kind)
                            .map(|b| (b.id.clone(), format!("{} ({})", b.name, b.output_file)))
                            .collect()
                    })
                    .unwrap_or_default();

                ui.horizontal(|ui| {
                    if ui.button("+ Block").clicked() {
                        self.project_state.add_custom_code_block_to_active_scene(kind);
                        if let Some(scene) = self
                            .project_state
                            .manifest
                            .scenes
                            .get(self.project_state.active_scene_index)
                        {
                            if let Some(last) = scene
                                .custom_code_blocks
                                .iter()
                                .rev()
                                .find(|b| b.kind == kind)
                            {
                                self.selected_custom_code_id = last.id.clone();
                            }
                        }
                        self.touch_and_refresh_preview();
                    }
                    if ui.button("- Block").clicked() {
                        let selected_id = self.selected_custom_code_id.clone();
                        if let Some(scene) = self
                            .project_state
                            .manifest
                            .scenes
                            .get_mut(self.project_state.active_scene_index)
                        {
                            if let Some(idx) = scene
                                .custom_code_blocks
                                .iter()
                                .position(|b| b.kind == kind && b.id == selected_id)
                            {
                                scene.custom_code_blocks.remove(idx);
                                self.selected_custom_code_id.clear();
                                self.touch_and_refresh_preview();
                            }
                        }
                    }
                });

                ui.separator();
                for (id, label) in &rows {
                    if ui
                        .selectable_label(*id == self.selected_custom_code_id, label)
                        .clicked()
                    {
                        self.selected_custom_code_id = id.clone();
                    }
                }

                if self.selected_custom_code_id.is_empty() {
                    return;
                }

                ui.separator();
                let mut changed = false;
                let mut helper_var_name = self.helper_var_name.clone();
                let mut helper_var_value = self.helper_var_value.clone();
                let mut helper_var_type = self.helper_var_type;
                let mut ensure_terrain_guard = false;
                let mut ensure_grapple_guard = false;
                let mut ensure_save_game_guard = false;
                let mut ensure_background_guard = false;
                let selected_id = self.selected_custom_code_id.clone();
                let Some(scene) = self
                    .project_state
                    .manifest
                    .scenes
                    .get_mut(self.project_state.active_scene_index)
                else {
                    return;
                };
                let Some(block) = scene
                    .custom_code_blocks
                    .iter_mut()
                    .find(|b| b.id == selected_id && b.kind == kind)
                else {
                    return;
                };

                changed |= ui.text_edit_singleline(&mut block.name).changed();
                changed |= Self::source_file_picker(
                    ui,
                    "File Target",
                    project_root.as_deref(),
                    &mut self.status_line,
                    &mut block.output_file,
                );
                if matches!(kind, CustomCodeKind::CustomEvents) {
                    ui.label("custom event name");
                    changed |= ui.text_edit_singleline(&mut block.custom_event_name).changed();
                }

                ui.collapsing("Quartz Syntax Helpers", |ui| {
                    if matches!(kind, CustomCodeKind::GameStateVars | CustomCodeKind::TypedVars | CustomCodeKind::UpdateLoops | CustomCodeKind::CustomEvents) {
                        ui.horizontal(|ui| {
                            ui.label("game_var");
                            ui.text_edit_singleline(&mut helper_var_name);
                            ui.label("value");
                            ui.text_edit_singleline(&mut helper_var_value);
                        });
                        ui.horizontal(|ui| {
                            ui.label("type");
                            egui::ComboBox::from_id_salt(format!("helper_var_type_{title}"))
                                .selected_text(helper_var_type.as_str())
                                .show_ui(ui, |ui| {
                                    ui.selectable_value(&mut helper_var_type, HelperVarType::I32, "i32");
                                    ui.selectable_value(&mut helper_var_type, HelperVarType::F32, "f32");
                                    ui.selectable_value(&mut helper_var_type, HelperVarType::Bool, "bool");
                                    ui.selectable_value(&mut helper_var_type, HelperVarType::Str, "string");
                                });
                        });
                    }

                    ui.collapsing("1) Variables", |ui| {
                        if matches!(kind, CustomCodeKind::GameStateVars | CustomCodeKind::TypedVars | CustomCodeKind::UpdateLoops | CustomCodeKind::CustomEvents) {
                            if ui.button("set_var").clicked() {
                                let expr = Self::quartz_value_expr(helper_var_type, &helper_var_value);
                                let snippet = format!("canvas.set_var({:?}, {});", helper_var_name.trim(), expr);
                                Self::append_code_block(&mut block.code, &snippet);
                                changed = true;
                            }
                            if ui.button("typed getter").clicked() {
                                let getter = Self::quartz_var_getter_name(helper_var_type);
                                let snippet = format!(
                                    "let {} = canvas.{}({:?});",
                                    helper_var_name.trim().replace(' ', "_"),
                                    getter,
                                    helper_var_name.trim()
                                );
                                Self::append_code_block(&mut block.code, &snippet);
                                changed = true;
                            }
                            if ui.button("get_var match").clicked() {
                                let snippet = format!(
                                    "if let Some(value) = canvas.get_var({:?}) {{\n    // match value variants here\n}}",
                                    helper_var_name.trim()
                                );
                                Self::append_code_block(&mut block.code, &snippet);
                                changed = true;
                            }
                            if ui.button("has/remove var").clicked() {
                                let snippet = format!(
                                    "if canvas.has_var({:?}) {{\n    canvas.remove_var({:?});\n}}",
                                    helper_var_name.trim(),
                                    helper_var_name.trim()
                                );
                                Self::append_code_block(&mut block.code, &snippet);
                                changed = true;
                            }
                            if ui.button("modify typed var").clicked() {
                                let modify_fn = match helper_var_type {
                                    HelperVarType::I32 => "modify_i32",
                                    HelperVarType::F32 => "modify_f32",
                                    HelperVarType::Bool => "modify_bool",
                                    HelperVarType::Str => "modify_str",
                                };
                                let snippet = format!(
                                    "canvas.{}({:?}, |v| {{\n    // return updated value\n    v\n}});",
                                    modify_fn,
                                    helper_var_name.trim()
                                );
                                Self::append_code_block(&mut block.code, &snippet);
                                changed = true;
                            }
                        }
                    });

                    ui.collapsing("2) Update & Events", |ui| {
                        if matches!(kind, CustomCodeKind::UpdateLoops) {
                            if ui.button("prime update body").clicked() {
                                let snippet = "// Called each tick via canvas.on_update generated wrapper\n// Put frame logic here";
                                Self::append_code_block(&mut block.code, snippet);
                                changed = true;
                            }
                            if ui.button("trigger custom action").clicked() {
                                Self::append_code_block(
                                    &mut block.code,
                                    "canvas.run(Action::custom(\"event_name\"));",
                                );
                                changed = true;
                            }
                        }
                        if matches!(kind, CustomCodeKind::CustomEvents) {
                            if ui.button("prime event body").clicked() {
                                let snippet = "// Event handler body\n// Trigger from update: canvas.run(Action::custom(\"event_name\"));";
                                Self::append_code_block(&mut block.code, snippet);
                                changed = true;
                            }
                        }
                    });

                    ui.collapsing("3) Core Actions", |ui| {
                        if matches!(kind, CustomCodeKind::UpdateLoops | CustomCodeKind::CustomEvents | CustomCodeKind::TopLevel) {
                            for snippet in [
                                "canvas.run(Action::show(Target::name(\"player\")));",
                                "canvas.run(Action::hide(Target::name(\"player\")));",
                                "canvas.run(Action::teleport(Target::name(\"player\"), Location::at(0.0, 0.0)));",
                                "canvas.run(Action::apply_momentum(Target::name(\"player\"), 0.0, 12.0));",
                                "canvas.run(Action::set_collision_layer(Target::name(\"player\"), 1));",
                            ] {
                                if ui.button(snippet).clicked() {
                                    Self::append_code_block(&mut block.code, snippet);
                                    changed = true;
                                }
                            }
                        }
                    });

                    ui.collapsing("4) Camera & FX", |ui| {
                        if matches!(kind, CustomCodeKind::UpdateLoops | CustomCodeKind::CustomEvents | CustomCodeKind::TopLevel) {
                            for snippet in [
                                "canvas.run(Action::smooth_zoom(1.15));",
                                "canvas.run(Action::camera_shake(8.0, 0.18));",
                                "canvas.run(Action::camera_flash(Color(255, 240, 120, 220), 0.12));",
                                "if let Some(cam) = canvas.camera_mut() { cam.smooth_zoom(1.1); }",
                            ] {
                                if ui.button(snippet).clicked() {
                                    Self::append_code_block(&mut block.code, snippet);
                                    changed = true;
                                }
                            }
                        }
                    });

                    ui.collapsing("5) Crystalline & Physics", |ui| {
                        if matches!(kind, CustomCodeKind::UpdateLoops | CustomCodeKind::CustomEvents | CustomCodeKind::TopLevel) {
                            for snippet in [
                                "canvas.run(Action::enable_crystalline());",
                                "canvas.run(Action::set_gravity(Target::name(\"player\"), 0.0));",
                                "canvas.run(Action::apply_force(Target::name(\"player\"), 10.0, -4.0));",
                                "canvas.run(Action::set_align_to_slope(Target::name(\"player\"), true));",
                            ] {
                                if ui.button(snippet).clicked() {
                                    Self::append_code_block(&mut block.code, snippet);
                                    changed = true;
                                }
                            }
                        }
                    });

                    ui.collapsing("6) Particles", |ui| {
                        if matches!(kind, CustomCodeKind::UpdateLoops | CustomCodeKind::CustomEvents | CustomCodeKind::TopLevel) {
                            for snippet in [
                                "canvas.run(Action::set_emitter_rate(\"trail\", 90.0));",
                                "canvas.run(Action::set_emitter_lifetime(\"trail\", 0.8));",
                                "canvas.run(Action::set_emitter_velocity(\"trail\", 0.0, -8.0));",
                                "canvas.run(Action::set_emitter_color(\"trail\", 255, 200, 120, 255));",
                            ] {
                                if ui.button(snippet).clicked() {
                                    Self::append_code_block(&mut block.code, snippet);
                                    changed = true;
                                }
                            }
                        }
                    });

                    ui.collapsing("7) Plugin Integration", |ui| {
                        if matches!(kind, CustomCodeKind::UpdateLoops | CustomCodeKind::CustomEvents | CustomCodeKind::TopLevel) {
                            ui.label("Terrain Collision Plugin");
                            for snippet in [
                                "if let Some(plugin) = canvas.get_plugin_mut::<TerrainCollisionPlugin>() {\n    plugin.register_terrain(\n        \"ground\",\n        include_bytes!(\"../../assets/ground.rgba\"),\n        (64, 64),\n        (64.0, 64.0),\n        128,\n        4.0,\n    );\n}",
                                "if let Some(plugin) = canvas.get_plugin_mut::<TerrainCollisionPlugin>() {\n    plugin.register_group_member(\n        \"floor\",\n        \"floor_tile_0\",\n        include_bytes!(\"../../assets/floor_tile.rgba\"),\n        (32, 32),\n        (32.0, 32.0),\n        128,\n        4.0,\n    );\n}",
                                "canvas.run(Action::PluginCall {\n    name: \"terrain_collision\".to_owned(),\n    payload: std::sync::Arc::new(TerrainCollisionCall::EnsureDynamicOutlineForImage {\n        name: \"player\".to_owned(),\n        rgba_bytes: frame_rgba_bytes.clone(),\n        sprite_dims: (32, 48),\n        object_size: (32.0, 48.0),\n        threshold: 1,\n        rdp_epsilon: 2.0,\n    }),\n});",
                                "canvas.run(Action::PluginCall {\n    name: \"terrain_collision\".to_owned(),\n    payload: std::sync::Arc::new(TerrainCollisionCall::UnregisterDynamicOutline {\n        name: \"player\".to_owned(),\n    }),\n});",
                                "canvas.run(Action::run_plugin(\"terrain_collision\", \"remove_group:floor\"));",
                                "canvas.run(Action::run_plugin(\"terrain_collision\", \"rebuild:floor\"));",
                            ] {
                                if ui.button(snippet).clicked() {
                                    ensure_terrain_guard = true;
                                    Self::append_code_block(&mut block.code, snippet);
                                    changed = true;
                                }
                            }

                            ui.separator();
                            ui.label("Grapple Plugin");
                            for snippet in [
                                "canvas.run(Action::PluginCall {\n    name: \"grapple\".to_owned(),\n    payload: std::sync::Arc::new(GrappleCommand::Attach {\n        target: Target::name(\"player\"),\n        grapple: GrappleConstraint::grappling_hook((anchor_x, anchor_y), 260.0),\n    }),\n});",
                                "canvas.run(Action::PluginCall {\n    name: \"grapple\".to_owned(),\n    payload: std::sync::Arc::new(GrappleCommand::SetLength {\n        target: Target::name(\"player\"),\n        value: 220.0,\n    }),\n});",
                                "canvas.run(Action::PluginCall {\n    name: \"grapple\".to_owned(),\n    payload: std::sync::Arc::new(GrappleCommand::SetAnchorObject {\n        target: Target::name(\"player\"),\n        anchor_object: \"hook_1\".to_owned(),\n    }),\n});",
                                "canvas.run(Action::PluginCall {\n    name: \"grapple\".to_owned(),\n    payload: std::sync::Arc::new(GrappleCommand::SetSwingBias {\n        target: Target::name(\"player\"),\n        bias: SwingBias::Horizontal,\n    }),\n});",
                                "canvas.run(Action::PluginCall {\n    name: \"grapple\".to_owned(),\n    payload: std::sync::Arc::new(GrappleCommand::Release {\n        target: Target::name(\"player\"),\n    }),\n});",
                            ] {
                                if ui.button(snippet).clicked() {
                                    ensure_grapple_guard = true;
                                    Self::append_code_block(&mut block.code, snippet);
                                    changed = true;
                                }
                            }

                            ui.separator();
                            ui.label("SaveGame Plugin");
                            for snippet in [
                                "if !matches!(canvas.get_var(\"save_game_registered\"), Some(Value::Bool(true))) {\n    canvas.add_plugin(SaveGamePlugin::default());\n    canvas.set_var(\"save_game_registered\", Value::Bool(true));\n}",
                                "canvas.run(Action::run_plugin(\"save_game\", \"save_slot:slot1\"));",
                            ] {
                                if ui.button(snippet).clicked() {
                                    ensure_save_game_guard = true;
                                    Self::append_code_block(&mut block.code, snippet);
                                    changed = true;
                                }
                            }

                            ui.separator();
                            ui.label("Background Plugin");
                            for snippet in [
                                "if !matches!(canvas.get_var(\"background_plugin_registered\"), Some(Value::Bool(true))) {\n    canvas.add_plugin(BackgroundPlugin::new());\n    canvas.set_var(\"background_plugin_registered\", Value::Bool(true));\n}",
                                "canvas.run(Action::run_plugin(\"background\", \"show:main_bg\"));",
                            ] {
                                if ui.button(snippet).clicked() {
                                    ensure_background_guard = true;
                                    Self::append_code_block(&mut block.code, snippet);
                                    changed = true;
                                }
                            }

                            ui.separator();
                            let generic = "canvas.run(Action::run_plugin(\"save_game\", \"save_slot:slot1\"));";
                            if ui.button(generic).clicked() {
                                ensure_save_game_guard = true;
                                Self::append_code_block(&mut block.code, generic);
                                changed = true;
                            }
                        }
                    });

                    ui.collapsing("8) Targets, Locations, Conditions", |ui| {
                        if matches!(kind, CustomCodeKind::UpdateLoops | CustomCodeKind::CustomEvents | CustomCodeKind::TopLevel) {
                            for snippet in [
                                "let t = Target::name(\"player\");",
                                "let l = Location::on_target(Target::name(\"player\"), Anchor::Center, (0.0, -24.0));",
                                "let cond = Condition::VarExists(\"score\".to_owned()).and(Condition::KeyHeld(Key::Space));",
                                "canvas.run(Action::when_if(cond, Action::custom(\"do_release\")));",
                            ] {
                                if ui.button(snippet).clicked() {
                                    Self::append_code_block(&mut block.code, snippet);
                                    changed = true;
                                }
                            }
                        }
                    });

                    ui.collapsing("9) Caches & Pools", |ui| {
                        if matches!(kind, CustomCodeKind::UpdateLoops | CustomCodeKind::CustomEvents | CustomCodeKind::TopLevel) {
                            for snippet in [
                                "let bullet_template = GameObject::build(\"bullet_tpl\").size(24.0, 24.0).build(canvas);\ncanvas.create_pool(\"bullet_pool\", bullet_template, 32);",
                                "if let Some(name) = canvas.pool_acquire(\"bullet_pool\", (player_x, player_y)) {\n    // configure the pooled object if needed\n}",
                                "canvas.pool_release(\"bullet_17\");\ncanvas.pool_release_all(\"bullet_pool\");",
                                "let available = canvas.pool_available(\"bullet_pool\");\nlet active = canvas.pool_active(\"bullet_pool\");",
                                "let cached = canvas.load_image_cached(\"assets/sprites/player.png\");",
                                "let cached_sized = canvas.load_image_sized_cached(\"assets/sprites/tile.png\", 128.0, 128.0);",
                                "let procedural = canvas.get_or_create_image(\"noise_bg\", || {\n    // build and return an Image here\n    canvas.load_image_cached(\"assets/sprites/fallback.png\")\n});",
                                "canvas.clear_image_cache();",
                            ] {
                                if ui.button(snippet).clicked() {
                                    Self::append_code_block(&mut block.code, snippet);
                                    changed = true;
                                }
                            }
                        }
                    });

                    ui.collapsing("10) Text & UI", |ui| {
                        if matches!(kind, CustomCodeKind::UpdateLoops | CustomCodeKind::CustomEvents | CustomCodeKind::TopLevel) {
                            for snippet in [
                                "static HUD_FONT: std::sync::OnceLock<Font> = std::sync::OnceLock::new();\nlet font = std::sync::Arc::new(\n    HUD_FONT\n        .get_or_init(|| Font::from_bytes(include_bytes!(\"../../assets/font.ttf\")).expect(\"hud font\"))\n        .clone(),\n);\nlet hud_text = Text::new(\n    vec![Span::new(\"Ready\".to_owned(), 30.0, Some(37.5), font, Color(255, 255, 255, 255), 0.0)],\n    None,\n    Align::Left,\n    None,\n);\ncanvas.run(Action::set_text(Target::name(\"hud_label\"), hud_text));",
                                "static STATUS_FONT: std::sync::OnceLock<Font> = std::sync::OnceLock::new();\nlet font = std::sync::Arc::new(\n    STATUS_FONT\n        .get_or_init(|| Font::from_bytes(include_bytes!(\"../../assets/font.ttf\")).expect(\"status font\"))\n        .clone(),\n);\nlet status_text = Text::new(\n    vec![\n        Span::new(\"HP: \".to_owned(), 22.0, Some(27.5), font.clone(), Color(255, 230, 120, 255), 0.0),\n        Span::new(\"100\".to_owned(), 22.0, Some(27.5), font, Color(255, 255, 255, 255), 0.0),\n    ],\n    None,\n    Align::Left,\n    None,\n);\ncanvas.run(Action::set_text(Target::name(\"hud_status\"), status_text));",
                                "// Build the Text first, then swap the drawable on the object.\nstatic SCORE_FONT: std::sync::OnceLock<Font> = std::sync::OnceLock::new();\nlet font = std::sync::Arc::new(\n    SCORE_FONT\n        .get_or_init(|| Font::from_bytes(include_bytes!(\"../../assets/font.ttf\")).expect(\"score font\"))\n        .clone(),\n);\nlet score_text = Text::new(\n    vec![Span::new(\"Score: 0\".to_owned(), 28.0, Some(35.0), font, Color(210, 255, 210, 255), 0.0)],\n    None,\n    Align::Left,\n    None,\n);\nif let Some(obj) = canvas.get_game_object_mut(\"score_label\") {\n    obj.set_drawable(Box::new(score_text));\n}",
                            ] {
                                if ui.button(snippet).clicked() {
                                    Self::append_code_block(&mut block.code, snippet);
                                    changed = true;
                                }
                            }
                        }
                    });

                    ui.collapsing("11) Sound", |ui| {
                        if matches!(kind, CustomCodeKind::UpdateLoops | CustomCodeKind::CustomEvents | CustomCodeKind::TopLevel) {
                            for snippet in [
                                "canvas.play_sound(\"assets/audio/click.ogg\");",
                                "canvas.play_sound_with(\"assets/audio/coin.ogg\", SoundOptions::new().volume(0.25));",
                                "canvas.run(Action::play_sound(\"assets/audio/whoosh.ogg\"));",
                                "canvas.run(Action::play_sound_with_options(\"assets/audio/ambience.ogg\", SoundOptions::new().volume(0.18).looping(true)));",
                            ] {
                                if ui.button(snippet).clicked() {
                                    Self::append_code_block(&mut block.code, snippet);
                                    changed = true;
                                }
                            }
                        }
                    });

                    ui.collapsing("12) Constants Templates", |ui| {
                        if matches!(kind, CustomCodeKind::Constants) {
                            for snippet in [
                                "pub const PLAYER_SPEED: f32 = 14.0;",
                                "pub const JUMP_FORCE: f32 = 22.0;",
                                "pub const HUD_PADDING: f32 = 18.0;",
                                "pub const PLAYER_TAG: &str = \"player\";",
                            ] {
                                if ui.button(snippet).clicked() {
                                    Self::append_code_block(&mut block.code, snippet);
                                    changed = true;
                                }
                            }
                        }
                    });

                    ui.collapsing("13) Intercode Relationships", |ui| {
                        ui.label("update loop -> canvas.run(Action::custom(name)) -> custom event handler via register_custom_event");
                        ui.label("game vars live on Canvas.game_vars and are shared between setup, update, and event handlers");
                        ui.label("plugins should be dispatched via Action::run_plugin or Action::PluginCall; avoid direct plugin calls in update loops");
                        ui.label("most runtime changes should flow through Action constructors (Quartz does dispatch + plugin hooks + physics ordering)");
                    });
                });

                let mut layouter = |ui: &egui::Ui, text: &str, wrap_width: f32| {
                    code_layouter(ui, text, wrap_width)
                };
                ui.label("Block Body (editable)");
                changed |= ui
                    .add(
                        TextEdit::multiline(&mut block.code)
                            .desired_rows(16)
                            .code_editor()
                            .layouter(&mut layouter),
                    )
                    .changed();

                ui.separator();
                ui.label("Generated Code Preview (editable)");
                let mut generated_preview = Self::generated_custom_block_preview(block);
                let mut preview_layouter = |ui: &egui::Ui, text: &str, wrap_width: f32| {
                    code_layouter(ui, text, wrap_width)
                };
                ui.add(
                    TextEdit::multiline(&mut generated_preview)
                        .desired_rows(12)
                        .code_editor()
                        .layouter(&mut preview_layouter),
                );
                if ui.button("Apply Preview Edits To Block Body").clicked() {
                    block.code = Self::try_extract_body_from_generated_preview(kind, &generated_preview);
                    changed = true;
                }

                let _ = block;
                let _ = scene;

                if ensure_terrain_guard {
                    self.ensure_plugin_imports_guard(
                        "terrain_collision",
                        &["use quartz::plugin::terrain_collision::{TerrainCollisionPlugin, TerrainCollisionCall};"],
                    );
                }
                if ensure_grapple_guard {
                    self.ensure_plugin_imports_guard(
                        "grapple",
                        &["use quartz::plugin::grapple::{GrappleCommand, GrappleConstraint, SwingBias};"],
                    );
                }
                if ensure_save_game_guard {
                    self.ensure_plugin_imports_guard(
                        "save_game",
                        &["use quartz::plugin::save_game::SaveGamePlugin;"],
                    );
                }
                if ensure_background_guard {
                    self.ensure_plugin_imports_guard(
                        "background",
                        &["use quartz::plugin::background::BackgroundPlugin;"],
                    );
                }

                self.helper_var_name = helper_var_name;
                self.helper_var_value = helper_var_value;
                self.helper_var_type = helper_var_type;

                if changed {
                    self.touch_and_refresh_preview();
                }
            });
    }

    fn grapple_wizard_window(&mut self, ctx: &egui::Context) {
        egui::Window::new("Grapple Wizard")
            .resizable(true)
            .default_size(egui::vec2(540.0, 520.0))
            .show(ctx, |ui| {
                let object_ids: Vec<String> = self
                    .project_state
                    .manifest
                    .scenes
                    .get(self.project_state.active_scene_index)
                    .map(|scene| scene.objects.iter().map(|o| o.id.clone()).collect())
                    .unwrap_or_default();

                if object_ids.is_empty() {
                    ui.label("Add objects to the scene before configuring grapple mechanics.");
                    return;
                }

                if self.grapple_target_object_id.trim().is_empty() {
                    self.grapple_target_object_id = object_ids[0].clone();
                }

                ui.checkbox(&mut self.grapple_viz_enabled, "show grapple visualization in scene/camera viewers");

                egui::ComboBox::from_label("target object")
                    .selected_text(self.grapple_target_object_id.as_str())
                    .show_ui(ui, |ui| {
                        for id in &object_ids {
                            ui.selectable_value(&mut self.grapple_target_object_id, id.clone(), id);
                        }
                    });

                ui.separator();
                ui.checkbox(&mut self.grapple_use_anchor_object, "anchor to object");
                if self.grapple_use_anchor_object {
                    if self.grapple_anchor_object_id.trim().is_empty() {
                        self.grapple_anchor_object_id = object_ids[0].clone();
                    }
                    egui::ComboBox::from_label("anchor object")
                        .selected_text(self.grapple_anchor_object_id.as_str())
                        .show_ui(ui, |ui| {
                            for id in &object_ids {
                                if *id == self.grapple_target_object_id {
                                    continue;
                                }
                                ui.selectable_value(&mut self.grapple_anchor_object_id, id.clone(), id);
                            }
                        });
                } else {
                    ui.add(Slider::new(&mut self.grapple_anchor_x, -10000.0..=10000.0).text("anchor x"));
                    ui.add(Slider::new(&mut self.grapple_anchor_y, -10000.0..=10000.0).text("anchor y"));
                }

                ui.separator();
                ui.add(Slider::new(&mut self.grapple_length, 1.0..=6000.0).text("rope length"));
                ui.add(Slider::new(&mut self.grapple_stiffness, 0.0..=1.0).text("stiffness"));
                ui.add(Slider::new(&mut self.grapple_damping, 0.0..=1.0).text("damping"));
                ui.add(Slider::new(&mut self.grapple_max_swing_speed, 0.0..=3000.0).text("max swing speed (0 = unlimited)"));
                ui.checkbox(&mut self.grapple_auto_shorten, "auto shorten");

                egui::ComboBox::from_label("swing bias")
                    .selected_text(self.grapple_bias.as_str())
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut self.grapple_bias, GrappleBiasOption::None, "None");
                        ui.selectable_value(&mut self.grapple_bias, GrappleBiasOption::Horizontal, "Horizontal");
                        ui.selectable_value(&mut self.grapple_bias, GrappleBiasOption::Vertical, "Vertical");
                    });

                ui.separator();
                ui.label("Typed API snippets (Action::PluginCall / GrappleCommand)");
                let attach_snippet = self.build_grapple_attach_snippet();
                if ui.button("Insert Attach Snippet").clicked() {
                    self.ensure_plugin_imports_guard(
                        "grapple",
                        &["use quartz::plugin::grapple::{GrappleCommand, GrappleConstraint, SwingBias};"],
                    );
                    self.insert_snippet_into_best_custom_block(&attach_snippet);
                    self.status_line = "Inserted grapple attach snippet into custom code block.".to_owned();
                }

                let set_length_snippet = format!(
                    "canvas.run(Action::PluginCall {{\n    name: \"grapple\".to_owned(),\n    payload: std::sync::Arc::new(GrappleCommand::SetLength {{\n        target: Target::name(\"{}\"),\n        value: {},\n    }}),\n}});",
                    self.grapple_target_object_id,
                    self.grapple_length
                );
                if ui.button("Insert SetLength Snippet").clicked() {
                    self.ensure_plugin_imports_guard(
                        "grapple",
                        &["use quartz::plugin::grapple::{GrappleCommand, GrappleConstraint, SwingBias};"],
                    );
                    self.insert_snippet_into_best_custom_block(&set_length_snippet);
                    self.status_line = "Inserted grapple SetLength snippet.".to_owned();
                }

                let release_snippet = format!(
                    "canvas.run(Action::PluginCall {{\n    name: \"grapple\".to_owned(),\n    payload: std::sync::Arc::new(GrappleCommand::Release {{\n        target: Target::name(\"{}\"),\n    }}),\n}});",
                    self.grapple_target_object_id
                );
                if ui.button("Insert Release Snippet").clicked() {
                    self.ensure_plugin_imports_guard(
                        "grapple",
                        &["use quartz::plugin::grapple::{GrappleCommand, GrappleConstraint, SwingBias};"],
                    );
                    self.insert_snippet_into_best_custom_block(&release_snippet);
                    self.status_line = "Inserted grapple Release snippet.".to_owned();
                }

                ui.collapsing("Current Attach Snippet Preview", |ui| {
                    let mut preview = attach_snippet.clone();
                    ui.add(TextEdit::multiline(&mut preview).desired_rows(10).code_editor());
                });

                ui.label("Syntax note: requires quartz::prelude::* and plugin 'grapple' registered on canvas.");
            });
    }

    fn generated_file_browser_window(&mut self, ctx: &egui::Context) {
        egui::Window::new("Generated File Browser")
            .resizable(true)
            .default_size(egui::vec2(860.0, 520.0))
            .show(ctx, |ui| {
                let Some(root) = self.project_root.clone() else {
                    ui.label("Open a project to browse generated files.");
                    return;
                };

                let refresh_needed = self.file_browser_cached_root.as_ref() != Some(&root)
                    || self.file_browser_cached_files.is_empty();
                if refresh_needed {
                    self.file_browser_cached_files = Self::collect_editable_files(&root);
                    self.file_browser_cached_root = Some(root.clone());
                }

                ui.horizontal(|ui| {
                    if ui.button("Refresh").clicked() {
                        self.file_browser_cached_files = Self::collect_editable_files(&root);
                        self.file_browser_cached_root = Some(root.clone());
                    }
                    ui.label("Manual edits can be tracked as custom overrides.");
                });

                let files = self.file_browser_cached_files.clone();

                ui.columns(2, |cols| {
                    egui::ScrollArea::vertical().id_salt("generated_file_browser_list").show(&mut cols[0], |ui| {
                        for rel in &files {
                            if ui
                                .selectable_label(*rel == self.file_browser_selected_rel, rel)
                                .clicked()
                            {
                                self.file_browser_selected_rel = rel.clone();
                                let path = root.join(rel);
                                self.file_browser_editor_text = std::fs::read_to_string(&path).unwrap_or_default();
                                self.file_browser_editor_dirty = false;
                            }
                        }
                    });

                    egui::ScrollArea::vertical().id_salt("generated_file_browser_editor").show(&mut cols[1], |ui| {
                        if self.file_browser_selected_rel.is_empty() {
                            ui.label("Select a file to edit.");
                            return;
                        }
                        ui.label(format!("Editing: {}", self.file_browser_selected_rel));
                        let mut layouter = |ui: &egui::Ui, text: &str, wrap_width: f32| {
                            code_layouter(ui, text, wrap_width)
                        };
                        self.file_browser_editor_dirty |= ui
                            .add(
                                TextEdit::multiline(&mut self.file_browser_editor_text)
                                    .desired_rows(24)
                                    .code_editor()
                                    .layouter(&mut layouter),
                            )
                            .changed();

                        ui.checkbox(
                            &mut self.file_browser_confirm_track_manual,
                            "Track this save as manual override in project metadata",
                        );
                        if ui.button("Save File").clicked() {
                            let path = root.join(&self.file_browser_selected_rel);
                            match std::fs::write(&path, &self.file_browser_editor_text) {
                                Ok(()) => {
                                    self.file_browser_editor_dirty = false;
                                    self.status_line = format!("Saved {}", path.display());
                                    if self.file_browser_confirm_track_manual {
                                        let rel = self.file_browser_selected_rel.clone();
                                        let text = self.file_browser_editor_text.clone();
                                        self.track_manual_override_for_file(&rel, &text);
                                    }
                                }
                                Err(err) => {
                                    self.status_line = format!("Failed to save file: {err}");
                                }
                            }
                        }
                        ui.label(
                            "Warning: manual edits may diverge from generated blocks. Use manual override tracking to retain them.",
                        );
                    });
                });
            });
    }

    fn track_manual_override_for_file(&mut self, rel_path: &str, content: &str) {
        let _ = self.project_state.track_manual_override_for_file(rel_path, content);
    }

    fn collect_editable_files(root: &Path) -> Vec<String> {
        let mut files = Self::collect_rs_files(root);
        for file in ["Cargo.toml", ".gitignore"] {
            let path = root.join(file);
            if path.exists() {
                files.push(file.to_owned());
            }
        }
        files.sort();
        files.dedup();
        files
    }

    fn objects_editor(&mut self, ui: &mut egui::Ui) {
        ui.heading("Object Builder");

        let object_rows: Vec<String> = self
            .project_state
            .manifest
            .scenes
            .get(self.project_state.active_scene_index)
            .map(|s| {
                s.objects
                    .iter()
                    .enumerate()
                    .map(|(i, o)| {
                        let lock = if o.lock_transform { " [LOCK]" } else { "" };
                        let cam = if o.advanced.is_camera_space_pinned() { " [CAM]" } else { "" };
                        let bg = if o.is_background { " [BG]" } else { "" };
                        let spawn = if o.spawn_only { " [SPAWN]" } else { "" };
                        format!("[{}] {} ({}){}{}{}{}", i + 1, o.name, o.template.as_str(), lock, cam, bg, spawn)
                    })
                    .collect()
            })
            .unwrap_or_default();

        for (idx, label) in object_rows.iter().enumerate() {
            let selected = idx == self.selected_object_index;
            if ui.selectable_label(selected, label).clicked() {
                self.selected_object_index = idx;
            }
        }

        ui.horizontal(|ui| {
            if ui.button("+ Object").clicked() {
                self.project_state.add_object_to_active_scene();
                let active_scene_index = self.project_state.active_scene_index;
                let snap_to_grid = self
                    .project_state
                    .manifest
                    .scenes
                    .get(active_scene_index)
                    .map(|scene| scene.canvas.snap_to_grid)
                    .unwrap_or(false);
                if let Some(scene) = self.project_state.manifest.scenes.get_mut(active_scene_index) {
                    if snap_to_grid {
                        if let Some(obj) = scene.objects.last_mut() {
                            obj.x = Self::snap_value(obj.x, self.grid_size);
                            obj.y = Self::snap_value(obj.y, self.grid_size);
                        }
                    }
                    self.selected_object_index = scene.objects.len().saturating_sub(1);
                }
            }
            if ui.button("+ Spawn Object").clicked() {
                self.project_state.add_spawn_only_object_to_active_scene();
                if let Some(scene) = self
                    .project_state
                    .manifest
                    .scenes
                    .get(self.project_state.active_scene_index)
                {
                    self.selected_object_index = scene.objects.len().saturating_sub(1);
                }
            }
            if ui.button("+ Background Object").clicked() {
                if let Some(scene) = self
                    .project_state
                    .manifest
                    .scenes
                    .get(self.project_state.active_scene_index)
                {
                    let (cell_w, cell_h) = Self::background_cell_size(&scene.canvas);
                    self.project_state
                        .add_background_object_to_active_scene(cell_w, cell_h);
                    if let Some(scene_mut) = self
                        .project_state
                        .manifest
                        .scenes
                        .get_mut(self.project_state.active_scene_index)
                    {
                        let canvas = scene_mut.canvas.clone();
                        if let Some(last) = scene_mut.objects.last_mut() {
                            Self::apply_background_snap_if_needed(last, &canvas);
                        }
                    }
                    if let Some(scene_mut) = self
                        .project_state
                        .manifest
                        .scenes
                        .get(self.project_state.active_scene_index)
                    {
                        self.selected_object_index = scene_mut.objects.len().saturating_sub(1);
                    }
                }
            }
            if ui.button("- Object").clicked() {
                if let Some(scene) = self
                    .project_state
                    .manifest
                    .scenes
                    .get_mut(self.project_state.active_scene_index)
                {
                    if !scene.objects.is_empty() && self.selected_object_index < scene.objects.len() {
                        scene.objects.remove(self.selected_object_index);
                        self.selected_object_index = self.selected_object_index.saturating_sub(1);
                        self.project_state.dirty = true;
                    }
                }
            }
        });

        ui.separator();
        let project_root = self.project_root.clone();
        let Some(scene) = self
            .project_state
            .manifest
            .scenes
            .get_mut(self.project_state.active_scene_index)
        else {
            return;
        };
        let Some(object) = scene.objects.get_mut(self.selected_object_index) else {
            ui.label("No object selected.");
            return;
        };

        ui.label("Basic");
        let mut changed = false;
        changed |= ui.text_edit_singleline(&mut object.id).changed();
        changed |= ui.text_edit_singleline(&mut object.name).changed();
        changed |= ui.checkbox(&mut object.is_background, "background object").changed();
        if object.is_background {
            object.spawn_only = false;
        }
        changed |= ui.checkbox(&mut object.spawn_only, "spawn-only object").changed();
        if object.spawn_only {
            object.is_background = false;
        }
        changed |= ui.checkbox(&mut object.lock_transform, "lock transform in canvas/editor").changed();
        ui.add_enabled_ui(!object.lock_transform, |ui| {
            changed |= ui.add(Slider::new(&mut object.x, -8000.0..=8000.0).text("x")).changed();
            changed |= ui.add(Slider::new(&mut object.y, -8000.0..=8000.0).text("y")).changed();
            changed |= ui.add(Slider::new(&mut object.w, 1.0..=4000.0).text("w")).changed();
            changed |= ui.add(Slider::new(&mut object.h, 1.0..=4000.0).text("h")).changed();
        });
        changed |= ui.add(Slider::new(&mut object.layer, -100..=100).text("layer")).changed();

        let mut space_mode = if object.advanced.is_camera_space_pinned() {
            "Camera Space (pinned)"
        } else {
            "World Space (default)"
        };
        egui::ComboBox::from_label("Space Mode")
            .selected_text(space_mode)
            .show_ui(ui, |ui| {
                changed |= ui
                    .selectable_value(&mut space_mode, "World Space (default)", "World Space (default)")
                    .changed();
                changed |= ui
                    .selectable_value(&mut space_mode, "Camera Space (pinned)", "Camera Space (pinned)")
                    .changed();
            });
        match space_mode {
            "Camera Space (pinned)" => {
                if !object.advanced.is_camera_space_pinned() {
                    object.advanced.set_camera_space_pinned(true);
                    changed = true;
                }
            }
            _ => {
                if object.advanced.is_camera_space_pinned() {
                    object.advanced.set_camera_space_pinned(false);
                    changed = true;
                }
            }
        }


        ui.separator();
        ui.label("Object Code Target");
        changed |= Self::source_file_picker(
            ui,
            "Object File Target",
            project_root.as_deref(),
            &mut self.status_line,
            &mut object.output_file,
        );
        ui.horizontal(|ui| {
            ui.label("Template");
            changed |= ui
                .selectable_value(
                    &mut object.template,
                    crate::core::quartz_domain::ObjectTemplate::Rectangle,
                    "Rectangle",
                )
                .changed();
            changed |= ui
                .selectable_value(
                    &mut object.template,
                    crate::core::quartz_domain::ObjectTemplate::Circle,
                    "Circle",
                )
                .changed();
        });

        ui.separator();
        ui.label("Visual Asset");
        let mut asset_mode = object.visual_asset_mode;
        egui::ComboBox::from_label("Object Visual Type")
            .selected_text(match asset_mode {
                ObjectVisualAssetMode::None => "None",
                ObjectVisualAssetMode::StaticImage => "Image",
                ObjectVisualAssetMode::AnimatedSprite => "Animated Sprite/GIF",
            })
            .show_ui(ui, |ui| {
                changed |= ui
                    .selectable_value(&mut asset_mode, ObjectVisualAssetMode::None, "None")
                    .changed();
                changed |= ui
                    .selectable_value(
                        &mut asset_mode,
                        ObjectVisualAssetMode::StaticImage,
                        "Image",
                    )
                    .changed();
                changed |= ui
                    .selectable_value(
                        &mut asset_mode,
                        ObjectVisualAssetMode::AnimatedSprite,
                        "Animated Sprite/GIF",
                    )
                    .changed();
            });
        if asset_mode != object.visual_asset_mode {
            object.visual_asset_mode = asset_mode;
            changed = true;
        }
        if object.visual_asset_mode != ObjectVisualAssetMode::None {
            changed |= Self::asset_file_picker(
                ui,
                "Visual Asset File",
                project_root.as_deref(),
                &mut self.status_line,
                &mut object.visual_asset_path,
            );
        }
        if object.visual_asset_mode == ObjectVisualAssetMode::StaticImage {
            changed |= ui
                .checkbox(&mut object.visual_asset_use_canvas_cache, "use Canvas image cache")
                .changed();
            if object.visual_asset_use_canvas_cache {
                ui.label("Cache Key (blank defaults to asset path)");
                changed |= ui
                    .text_edit_singleline(&mut object.visual_asset_cache_key)
                    .changed();
                changed |= ui
                    .checkbox(
                        &mut object.visual_asset_size_aware_cache,
                        "include object size in cache key",
                    )
                    .changed();
            }
        }
        if object.visual_asset_mode == ObjectVisualAssetMode::AnimatedSprite {
            changed |= ui
                .add(Slider::new(&mut object.visual_asset_fps, 1.0..=60.0).text("animation fps"))
                .changed();
        }

        let mut tag_string = object.tags.join(",");
        if ui.text_edit_singleline(&mut tag_string).changed() {
            object.tags = tag_string
                .split(',')
                .map(|s| s.trim().to_owned())
                .filter(|s| !s.is_empty())
                .collect();
            changed = true;
        }

        ui.separator();
        ui.label("Advanced Parameter Visibility");
        changed |= ui.checkbox(&mut object.visible.physics, "show physics params").changed();
        changed |= ui.checkbox(&mut object.visible.collision, "show collision params").changed();
        changed |= ui.checkbox(&mut object.visible.slope, "show slope/surface params").changed();
        changed |= ui.checkbox(&mut object.visible.planetary, "show planetary gravity params").changed();
        changed |= ui.checkbox(&mut object.visible.camera_space, "show camera-space params").changed();

        if object.visible.physics {
            ui.label("Physics");
            let mut preset = object.advanced.material.preset;
            egui::ComboBox::from_label("physics material preset")
                .selected_text(preset.as_str())
                .show_ui(ui, |ui| {
                    changed |= ui.selectable_value(&mut preset, ObjectPhysicsMaterialPreset::Default, "Default").changed();
                    changed |= ui.selectable_value(&mut preset, ObjectPhysicsMaterialPreset::Rubber, "Rubber").changed();
                    changed |= ui.selectable_value(&mut preset, ObjectPhysicsMaterialPreset::Ice, "Ice").changed();
                    changed |= ui.selectable_value(&mut preset, ObjectPhysicsMaterialPreset::Metal, "Metal").changed();
                    changed |= ui.selectable_value(&mut preset, ObjectPhysicsMaterialPreset::Wood, "Wood").changed();
                    changed |= ui.selectable_value(&mut preset, ObjectPhysicsMaterialPreset::Stone, "Stone").changed();
                    changed |= ui.selectable_value(&mut preset, ObjectPhysicsMaterialPreset::Bouncy, "Bouncy").changed();
                    changed |= ui.selectable_value(&mut preset, ObjectPhysicsMaterialPreset::Sticky, "Sticky").changed();
                    changed |= ui.selectable_value(&mut preset, ObjectPhysicsMaterialPreset::Glass, "Glass").changed();
                    changed |= ui.selectable_value(&mut preset, ObjectPhysicsMaterialPreset::Feather, "Feather").changed();
                    changed |= ui.selectable_value(&mut preset, ObjectPhysicsMaterialPreset::Custom, "Custom").changed();
                });
            if preset != object.advanced.material.preset {
                object.advanced.material = crate::core::quartz_domain::ObjectPhysicsMaterialSpec::resolved_defaults(preset);
                changed = true;
            }
            changed |= ui
                .add(Slider::new(&mut object.advanced.momentum_x, -100.0..=100.0).text("momentum x"))
                .changed();
            changed |= ui
                .add(Slider::new(&mut object.advanced.momentum_y, -100.0..=100.0).text("momentum y"))
                .changed();
            changed |= ui
                .add(Slider::new(&mut object.advanced.resistance_x, 0.0..=10.0).text("resistance x"))
                .changed();
            changed |= ui
                .add(Slider::new(&mut object.advanced.resistance_y, 0.0..=10.0).text("resistance y"))
                .changed();
            changed |= ui
                .add(Slider::new(&mut object.advanced.gravity, -20.0..=20.0).text("gravity"))
                .changed();
            changed |= ui
                .add(Slider::new(&mut object.advanced.rotation_deg, -360.0..=360.0).text("rotation"))
                .changed();
            changed |= ui
                .add(Slider::new(&mut object.advanced.pivot_x, 0.0..=1.0).text("pivot x"))
                .changed();
            changed |= ui
                .add(Slider::new(&mut object.advanced.pivot_y, 0.0..=1.0).text("pivot y"))
                .changed();


        if changed {
            let canvas = scene.canvas.clone();
            Self::apply_background_snap_if_needed(object, &canvas);
        }
            ui.separator();
            ui.label("Crystalline Physics Material");
            let custom_material = object.advanced.material.preset == ObjectPhysicsMaterialPreset::Custom;
            ui.add_enabled_ui(custom_material, |ui| {
                changed |= ui
                    .add(Slider::new(&mut object.advanced.material.elasticity, 0.0..=1.5).text("elasticity"))
                    .changed();
                changed |= ui
                    .add(Slider::new(&mut object.advanced.material.friction, 0.0..=2.0).text("friction"))
                    .changed();
                changed |= ui
                    .add(Slider::new(&mut object.advanced.material.density, 0.0..=10.0).text("density"))
                    .changed();
            });
            if !custom_material {
                ui.label("Preset materials export to Quartz as PhysicsMaterial::preset().");
            }
        }

        if object.visible.collision {
            ui.label("Collision");
            changed |= ui
                .add(Slider::new(&mut object.advanced.collision_layer, 0..=64).text("collision layer"))
                .changed();
            changed |= ui
                .add(Slider::new(&mut object.advanced.collision_mask, 0..=64).text("collision mask"))
                .changed();
        }

        if object.visible.slope {
            ui.separator();
            ui.label("Slope & Surface");
            changed |= ui.checkbox(&mut object.advanced.slope_enabled, "enable slope offsets").changed();
            if object.advanced.slope_enabled {
                changed |= ui
                    .add(Slider::new(&mut object.advanced.slope_left_offset, -1000.0..=1000.0).text("slope left offset"))
                    .changed();
                changed |= ui
                    .add(Slider::new(&mut object.advanced.slope_right_offset, -1000.0..=1000.0).text("slope right offset"))
                    .changed();
                changed |= ui
                    .checkbox(&mut object.advanced.slope_auto_rotation, "derive rotation from slope")
                    .changed();
            }
            changed |= ui.checkbox(&mut object.advanced.one_way, "one-way platform").changed();
            changed |= ui
                .checkbox(&mut object.advanced.surface_velocity_enabled, "surface conveyor velocity")
                .changed();
            if object.advanced.surface_velocity_enabled {
                changed |= ui
                    .add(Slider::new(&mut object.advanced.surface_velocity_x, -200.0..=200.0).text("surface velocity x"))
                    .changed();
            }
            changed |= ui
                .checkbox(&mut object.advanced.surface_normal_enabled, "custom surface normal")
                .changed();
            if object.advanced.surface_normal_enabled {
                changed |= ui
                    .add(Slider::new(&mut object.advanced.surface_normal_x, -1.0..=1.0).text("surface normal x"))
                    .changed();
                changed |= ui
                    .add(Slider::new(&mut object.advanced.surface_normal_y, -1.0..=1.0).text("surface normal y"))
                    .changed();
            }
            changed |= ui
                .checkbox(&mut object.advanced.align_to_slope, "align to slope")
                .changed();
            if object.advanced.align_to_slope {
                changed |= ui
                    .add(Slider::new(&mut object.advanced.align_to_slope_speed, 0.0..=64.0).text("align-to-slope speed"))
                    .changed();
            }
        }

        if object.visible.planetary {
            ui.separator();
            ui.label("Planetary Gravity & Wells");
            changed |= ui.checkbox(&mut object.advanced.planet_enabled, "planet collision body").changed();
            if object.advanced.planet_enabled {
                changed |= ui
                    .add(Slider::new(&mut object.advanced.planet_radius, 0.0..=8000.0).text("planet radius"))
                    .changed();
            }
            changed |= ui
                .checkbox(&mut object.advanced.gravity_target_enabled, "gravity target tag")
                .changed();
            if object.advanced.gravity_target_enabled {
                changed |= ui
                    .text_edit_singleline(&mut object.advanced.gravity_target_tag)
                    .changed();
            }
            changed |= ui
                .add(Slider::new(&mut object.advanced.gravity_strength, 0.0..=128.0).text("gravity strength"))
                .changed();
            changed |= ui
                .add(Slider::new(&mut object.advanced.gravity_influence_mult, 0.01..=100.0).text("gravity influence mult"))
                .changed();

            let mut falloff = object.advanced.gravity_falloff;
            egui::ComboBox::from_label("gravity falloff")
                .selected_text(falloff.as_str())
                .show_ui(ui, |ui| {
                    changed |= ui
                        .selectable_value(&mut falloff, QuartzGravityFalloff::Constant, "Constant")
                        .changed();
                    changed |= ui
                        .selectable_value(&mut falloff, QuartzGravityFalloff::Linear, "Linear")
                        .changed();
                    changed |= ui
                        .selectable_value(&mut falloff, QuartzGravityFalloff::InverseSquare, "InverseSquare")
                        .changed();
                });
            if falloff != object.advanced.gravity_falloff {
                object.advanced.gravity_falloff = falloff;
                changed = true;
            }

            changed |= ui
                .checkbox(&mut object.advanced.gravity_all_sources, "all gravity sources")
                .changed();
            changed |= ui
                .checkbox(&mut object.advanced.gravity_identity_enabled, "gravity identity")
                .changed();
            if object.advanced.gravity_identity_enabled {
                changed |= ui
                    .text_edit_singleline(&mut object.advanced.gravity_identity)
                    .changed();
            }
            changed |= ui.checkbox(&mut object.advanced.auto_align, "auto align").changed();
            if object.advanced.auto_align {
                changed |= ui
                    .add(Slider::new(&mut object.advanced.auto_align_speed, 0.0..=64.0).text("auto align speed"))
                    .changed();
            }
        }

        if object.visible.camera_space {
            ui.label("Camera-space / HUD");
            if object.advanced.is_camera_space_pinned() {
                ui.label("Pinned objects follow screen space and ignore camera zoom.");
            } else {
                changed |= ui
                    .checkbox(&mut object.advanced.ignore_zoom, "ignore zoom (still world-space)")
                    .changed();
            }
        }

        if changed {
            self.touch_and_refresh_preview();
        }
    }

    fn bottom_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(RichText::new("Status:").strong());
            ui.label(&self.status_line);
        });
        ui.horizontal(|ui| {
            ui.label("Preview message:");
            ui.label(&self.hot_reload.last_message);
            if let Some(cwd) = self.hot_reload.working_dir() {
                ui.label(format!("cwd: {}", cwd.display()));
            }
        });
    }

    fn create_project_interactive(&mut self) {
        let Some(path) = rfd::FileDialog::new().set_title("Select Quartz Forge project root").pick_folder() else {
            return;
        };

        let project_name = self.new_project_name.trim();
        if project_name.is_empty() {
            self.status_line = "Project name cannot be empty.".to_owned();
            return;
        }

        match persistence::create_new_project(project_name.to_owned(), &path) {
            Ok(state) => {
                self.project_state = state;
                self.project_root = Some(path.clone());
                self.quartz_preview = self.build_scene_source();
                self.project_sync_report = None;
                self.show_project_sync_prompt = false;
                self.status_line = format!("Created project at {}", path.display());
            }
            Err(err) => {
                self.status_line = format!("Create project failed: {err}");
            }
        }
    }

    fn open_project_interactive(&mut self) {
        let Some(path) = rfd::FileDialog::new().set_title("Open Quartz Forge project root").pick_folder() else {
            return;
        };

        match persistence::load_project_with_sync(&path) {
            Ok((state, report)) => {
                self.project_state = state;
                self.project_root = Some(path.clone());
                self.quartz_preview = self.build_scene_source();
                self.show_project_sync_prompt = report.needs_user_action();
                self.project_sync_report = if report.needs_user_action() {
                    Some(report.clone())
                } else {
                    None
                };
                if matches!(report.status, persistence::ProjectSyncStatus::MissingSnapshot) {
                    self.status_line = format!("Loaded project from {}. {}", path.display(), report.summary);
                } else if report.needs_user_action() {
                    self.status_line = format!("Loaded project from {}. {}", path.display(), report.summary);
                } else {
                    self.status_line = format!("Loaded project from {}", path.display());
                }
            }
            Err(err) => {
                self.status_line = format!("Open project failed: {err}");
            }
        }
    }

    fn save_project(&mut self) {
        let Some(root) = self.project_root.clone() else {
            self.status_line = "Set a project root via New/Open Project first.".to_owned();
            return;
        };

        match persistence::save_project(&mut self.project_state, &root) {
            Ok(()) => {
                self.status_line = format!("Saved project to {}", root.display());
                if let Ok(report) = persistence::validate_project_sync(&self.project_state, &root) {
                    if report.needs_user_action() {
                        self.project_sync_report = Some(report.clone());
                        self.show_project_sync_prompt = true;
                        self.status_line = format!(
                            "Saved project to {}. {}",
                            root.display(),
                            report.summary
                        );
                    }
                }
            }
            Err(err) => {
                self.status_line = format!("Save failed: {err}");
            }
        }
    }

    fn start_preview(&mut self) {
        let Some(root) = self.project_root.clone() else {
            self.status_line = "Open a project before starting preview.".to_owned();
            return;
        };

        match self.hot_reload.start_preview(&root) {
            Ok(()) => {
                self.status_line = "Preview process started.".to_owned();
            }
            Err(err) => {
                self.status_line = err;
            }
        }
    }

    fn stop_preview(&mut self) {
        if let Err(err) = self.hot_reload.stop_preview() {
            self.status_line = err;
            return;
        }
        self.status_line = "Preview process stopped.".to_owned();
    }

    #[allow(dead_code)]
    fn write_generated_files_for_scene(&mut self, root: &Path, scene_index: usize) -> Result<()> {
        let original_scene_index = self.project_state.active_scene_index;
        self.project_state.active_scene_index = scene_index;

        let result = (|| -> Result<()> {
            let scene = self
                .project_state
                .manifest
                .scenes
                .get(scene_index)
                .cloned()
                .ok_or_else(|| anyhow!("missing scene at index {scene_index}"))?;

            let configured_rel = scene.source_file.trim().to_owned();
            let fallback_rel = format!("scripts/{}", codegen::generated_file_name(&self.project_state));
            let rel_path = if configured_rel.is_empty() { fallback_rel } else { configured_rel };

            let out_path = root.join(&rel_path);
            if let Some(parent) = out_path.parent() {
                std::fs::create_dir_all(parent)
                    .with_context(|| format!("failed to prepare output directory {}", parent.display()))?;
            }

            let scene_output_generated = self.build_scene_source();
            let scene_override = scene
                .custom_code_blocks
                .iter()
                .find(|b| {
                    b.kind == CustomCodeKind::ManualFileOverride
                        && b.output_file.trim() == rel_path
                        && !b.code.trim().is_empty()
                })
                .map(|b| b.code.clone());
            let scene_output = scene_override.unwrap_or(scene_output_generated);

            std::fs::write(&out_path, scene_output)
                .with_context(|| format!("failed to write generated scene file {}", out_path.display()))?;

            let scene_source_file = scene.source_file.trim().to_owned();
            let mut target_files: Vec<String> = Vec::new();
            for object in &scene.objects {
                let target = Self::component_target_path(&scene_source_file, &object.output_file);
                if target != scene_source_file && !target_files.contains(&target) {
                    target_files.push(target);
                }
            }
            for event in &scene.events {
                let target = Self::component_target_path(&scene_source_file, &event.output_file);
                if target != scene_source_file && !target_files.contains(&target) {
                    target_files.push(target);
                }
            }
            for tree in &scene.logic_trees {
                let target = Self::component_target_path(&scene_source_file, &tree.output_file);
                if target != scene_source_file && !target_files.contains(&target) {
                    target_files.push(target);
                }
            }
            for block in &scene.custom_code_blocks {
                if block.kind == CustomCodeKind::ManualFileOverride {
                    let manual_target = block.output_file.trim();
                    if !manual_target.is_empty()
                        && manual_target != scene_source_file
                        && !target_files.contains(&manual_target.to_owned())
                    {
                        target_files.push(manual_target.to_owned());
                    }
                    continue;
                }
                let target = Self::component_target_path(&scene_source_file, &block.output_file);
                if target != scene_source_file && !target_files.contains(&target) {
                    target_files.push(target);
                }
            }

            for target_file in target_files {
                let module_override = scene
                    .custom_code_blocks
                    .iter()
                    .find(|b| {
                        b.kind == CustomCodeKind::ManualFileOverride
                            && b.output_file.trim() == target_file
                            && !b.code.trim().is_empty()
                    })
                    .map(|b| b.code.clone());

                if let Some(module_source) =
                    module_override.or_else(|| self.build_component_module_source(&target_file))
                {
                    let module_path = root.join(&target_file);
                    if let Some(parent) = module_path.parent() {
                        std::fs::create_dir_all(parent).with_context(|| {
                            format!("failed to prepare component directory {}", parent.display())
                        })?;
                    }
                    std::fs::write(&module_path, module_source).with_context(|| {
                        format!("failed to write component file {}", module_path.display())
                    })?;
                }
            }

            Ok(())
        })();

        self.project_state.active_scene_index = original_scene_index;
        self.quartz_preview = self.build_scene_source();
        result
    }

    fn write_all_generated_files_from_project_state(&mut self) {
        let Some(root) = self.project_root.clone() else {
            self.status_line = "Open a project before writing generated script.".to_owned();
            return;
        };

        if let Err(err) = persistence::ensure_runtime_scaffold(&self.project_state, &root) {
            self.status_line = format!("Failed to prepare runtime scaffold: {err}");
            return;
        }

        if let Err(err) = project_sync::write_all_generated_files_from_state(&self.project_state, &root) {
            self.status_line = format!("Failed to write generated files: {err}");
            return;
        }

        if let Err(err) = persistence::save_project(&mut self.project_state, &root) {
            self.status_line = format!("Generated files written, but auto-save failed: {err}");
            return;
        }

        if let Err(err) = persistence::write_sync_snapshot(&self.project_state, &root) {
            self.status_line = format!("Generated files and project saved, but sync snapshot failed: {err}");
            return;
        }

        match persistence::validate_project_sync(&self.project_state, &root) {
            Ok(report) => {
                self.project_sync_report = if report.needs_user_action() {
                    Some(report.clone())
                } else {
                    None
                };
                self.show_project_sync_prompt = report.needs_user_action();
                self.status_line = format!(
                    "Generated files written for all scenes and project auto-saved to {}.",
                    root.display()
                );
            }
            Err(err) => {
                self.status_line = format!(
                    "Generated files written and project auto-saved, but sync validation failed: {err}"
                );
            }
        }
    }

    fn write_generated_script(&mut self) {
        self.write_all_generated_files_from_project_state();
    }

    fn restore_project_state_from_last_export(&mut self) {
        let Some(root) = self.project_root.clone() else {
            self.status_line = "Open a project before reconciling project sync.".to_owned();
            return;
        };

        match persistence::restore_project_from_sync_snapshot(&root) {
            Ok(state) => {
                self.project_state = state;
                self.quartz_preview = self.build_scene_source();
                match persistence::save_project(&mut self.project_state, &root) {
                    Ok(()) => {
                        self.project_sync_report = None;
                        self.show_project_sync_prompt = false;
                        self.status_line = "Restored project save state from the last exported Quartz Forge file snapshot.".to_owned();
                    }
                    Err(err) => {
                        self.status_line = format!("Restored snapshot state in memory, but saving failed: {err}");
                    }
                }
            }
            Err(err) => {
                self.status_line = format!("Failed to restore project from sync snapshot: {err}");
            }
        }
    }

    fn import_files_as_manual_overrides(&mut self, rel_paths: &[String]) {
        let Some(root) = self.project_root.clone() else {
            self.status_line = "Open a project before importing manual overrides.".to_owned();
            return;
        };

        let mut imported = Vec::new();
        let mut failed = Vec::new();
        for rel_path in rel_paths {
            let path = root.join(rel_path);
            match std::fs::read_to_string(&path) {
                Ok(content) => {
                    if self.project_state.track_manual_override_for_file(rel_path, &content).is_some() {
                        imported.push(rel_path.clone());
                    } else {
                        failed.push(format!("{rel_path} (no owning scene could be resolved)"));
                    }
                }
                Err(err) => {
                    failed.push(format!("{rel_path} ({err})"));
                }
            }
        }

        if imported.is_empty() {
            self.status_line = if failed.is_empty() {
                "No files were imported as manual overrides.".to_owned()
            } else {
                format!("Failed to import manual overrides: {}", failed.join(", "))
            };
            return;
        }

        if let Err(err) = persistence::save_project(&mut self.project_state, &root) {
            self.status_line = format!("Imported overrides in memory, but saving project failed: {err}");
            return;
        }

        if let Err(err) = persistence::write_sync_snapshot(&self.project_state, &root) {
            self.status_line = format!("Imported overrides, but sync snapshot failed: {err}");
            return;
        }

        match persistence::validate_project_sync(&self.project_state, &root) {
            Ok(report) => {
                self.project_sync_report = if report.needs_user_action() {
                    Some(report.clone())
                } else {
                    None
                };
                self.show_project_sync_prompt = report.needs_user_action();
                self.status_line = if failed.is_empty() {
                    format!(
                        "Imported {} file(s) as ManualFileOverride and saved the project.",
                        imported.len()
                    )
                } else {
                    format!(
                        "Imported {} file(s) as ManualFileOverride; some files failed: {}",
                        imported.len(),
                        failed.join(", ")
                    )
                };
            }
            Err(err) => {
                self.status_line = format!("Imported overrides, but sync validation failed: {err}");
            }
        }
    }

    fn import_files_semantically(&mut self, rel_paths: &[String]) {
        let Some(root) = self.project_root.clone() else {
            self.status_line = "Open a project before running semantic import.".to_owned();
            return;
        };

        match project_import::import_files_into_state(&mut self.project_state, &root, rel_paths, true) {
            Ok(report) => {
                if let Err(err) = persistence::save_project(&mut self.project_state, &root) {
                    self.status_line = format!("Semantic import succeeded in memory, but saving project failed: {err}");
                    return;
                }
                if let Err(err) = persistence::write_sync_snapshot(&self.project_state, &root) {
                    self.status_line = format!("Semantic import saved project, but sync snapshot failed: {err}");
                    return;
                }
                match persistence::validate_project_sync(&self.project_state, &root) {
                    Ok(sync_report) => {
                        self.project_sync_report = if sync_report.needs_user_action() {
                            Some(sync_report.clone())
                        } else {
                            None
                        };
                        self.show_project_sync_prompt = sync_report.needs_user_action();
                        self.quartz_preview = self.build_scene_source();
                    }
                    Err(err) => {
                        self.status_line = format!("Semantic import saved project, but sync validation failed: {err}");
                        return;
                    }
                }

                let mut parts = Vec::new();
                if !report.imported_files.is_empty() {
                    parts.push(format!(
                        "semantically imported {} file(s), {} object(s), {} custom block(s)",
                        report.imported_files.len(),
                        report.imported_object_count,
                        report.imported_custom_block_count
                    ));
                }
                if !report.fallback_manual_override_files.is_empty() {
                    parts.push(format!(
                        "fell back to ManualFileOverride for {} file(s)",
                        report.fallback_manual_override_files.len()
                    ));
                }
                if !report.unsupported_files.is_empty() {
                    parts.push(format!(
                        "left {} file(s) unsupported",
                        report.unsupported_files.len()
                    ));
                }
                if parts.is_empty() {
                    self.status_line = "Semantic import made no project-state changes.".to_owned();
                } else {
                    self.status_line = format!("Semantic import: {}.", parts.join(", "));
                }
            }
            Err(err) => {
                self.status_line = format!("Semantic import failed: {err}");
            }
        }
    }

    fn background_cell_size(canvas: &SceneCanvasSpec) -> (f32, f32) {
        let vw = canvas.virtual_width.max(1.0);
        let vh = canvas.virtual_height.max(1.0);
        match canvas.orientation {
            CanvasOrientation::Landscape => (vw.max(vh), vw.min(vh)),
            CanvasOrientation::Portrait => (vw.min(vh), vw.max(vh)),
        }
    }

    fn import_asset_into_project(root: &Path, source: &Path, status_line: &mut String) -> Option<String> {
        if let Ok(rel) = source.strip_prefix(root) {
            return Some(rel.to_string_lossy().replace('\\', "/"));
        }

        let file_name = source.file_name()?.to_string_lossy().to_string();
        let dest_dir = root.join("assets");
        if let Err(err) = std::fs::create_dir_all(&dest_dir) {
            *status_line = format!("Failed to create assets directory: {err}");
            return None;
        }

        let mut dest = dest_dir.join(&file_name);
        if dest.exists() {
            let stem = source
                .file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_else(|| "asset".to_owned());
            let ext = source.extension().map(|e| e.to_string_lossy().to_string());
            let mut idx = 1;
            loop {
                let candidate = if let Some(ext) = &ext {
                    dest_dir.join(format!("{}_{}.{}", stem, idx, ext))
                } else {
                    dest_dir.join(format!("{}_{}", stem, idx))
                };
                if !candidate.exists() {
                    dest = candidate;
                    break;
                }
                idx += 1;
            }
        }

        if let Err(err) = std::fs::copy(source, &dest) {
            *status_line = format!("Failed to copy asset into project: {err}");
            return None;
        }

        if let Ok(rel) = dest.strip_prefix(root) {
            *status_line = format!("Imported asset: {}", rel.display());
            Some(rel.to_string_lossy().replace('\\', "/"))
        } else {
            None
        }
    }

    fn touch_and_refresh_preview(&mut self) {
        self.project_state.dirty = true;
        self.preview_refresh_pending = true;
    }

    fn flush_preview_refresh_if_due(&mut self, force: bool) {
        if !self.preview_refresh_pending {
            return;
        }
        let now = Instant::now();
        let due = force
            || self
                .last_preview_refresh_at
                .map(|at| now.duration_since(at) >= Duration::from_millis(120))
                .unwrap_or(true);
        if due {
            self.quartz_preview = self.build_scene_source();
            self.last_preview_refresh_at = Some(now);
            self.preview_refresh_pending = false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::QuartzForgeApp;

    #[test]
    fn component_module_path_attr_steps_out_of_scene_directory() {
        let actual = QuartzForgeApp::component_module_path_attr(
            "src/scripts/main_scene_scene.rs",
            "src/game_state.rs",
        );
        assert_eq!(actual, "../game_state.rs");
    }

    #[test]
    fn component_module_path_attr_preserves_nested_targets() {
        let actual = QuartzForgeApp::component_module_path_attr(
            "src/scripts/main_scene_scene.rs",
            "src/scripts/components/shared.rs",
        );
        assert_eq!(actual, "components/shared.rs");
    }
}

impl eframe::App for QuartzForgeApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.hot_reload.poll();
        self.flush_preview_refresh_if_due(false);

        let needs_fast_repaint = self.canvas_drag.is_some()
            || self.camera_view_drag.is_some();
        if needs_fast_repaint {
            ctx.request_repaint_after(Duration::from_millis(16));
        } else if self.active_scene_has_animated_assets() {
            ctx.request_repaint_after(Duration::from_millis(16));
        } else if self.preview_refresh_pending {
            ctx.request_repaint_after(Duration::from_millis(40));
        } else if matches!(self.hot_reload.state, PreviewState::Running) {
            ctx.request_repaint_after(Duration::from_millis(250));
        }

        if self.show_startup_prompt && self.project_root.is_none() {
            egui::Window::new("Create or Open Project")
                .anchor(Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
                    ui.label("Quartz Forge needs a project directory before saving or writing files.");
                    ui.label("Create a new project or open an existing one to continue.");
                    ui.separator();
                    ui.horizontal(|ui| {
                        if ui.button("New Project").clicked() {
                            self.create_project_interactive();
                            self.show_startup_prompt = false;
                        }
                        if ui.button("Open Project").clicked() {
                            self.open_project_interactive();
                            self.show_startup_prompt = false;
                        }
                    });
                    if ui.button("Continue Untitled").clicked() {
                        self.show_startup_prompt = false;
                    }
                });
        }

        if self.show_project_sync_prompt {
            if let Some(report) = self.project_sync_report.clone() {
                egui::Window::new("Project Sync Reconciliation")
                    .anchor(Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
                    .collapsible(false)
                    .resizable(true)
                    .default_width(620.0)
                    .show(ctx, |ui| {
                        ui.label(&report.summary);
                        if let Some(generated_at) = &report.snapshot_generated_at_utc {
                            ui.label(format!("Last exported sync snapshot: {generated_at}"));
                        }
                        ui.separator();
                        if !report.modified_files.is_empty() {
                            ui.label("Modified tracked files:");
                            for path in &report.modified_files {
                                ui.label(format!("- {path}"));
                            }
                        }
                        if !report.missing_files.is_empty() {
                            ui.label("Missing tracked files:");
                            for path in &report.missing_files {
                                ui.label(format!("- {path}"));
                            }
                        }
                        if !report.extra_files.is_empty() {
                            ui.label("Extra tracked files not present in the last export snapshot:");
                            for path in &report.extra_files {
                                ui.label(format!("- {path}"));
                            }
                        }
                        ui.separator();
                        ui.label(
                            "Use ManualFileOverride tracking for user-owned Rust edits you want Quartz Forge to preserve across regeneration. Untracked Rust edits cannot be imported back into scene/object/event data automatically.",
                        );
                        ui.separator();
                        let importable_files = report
                            .modified_files
                            .iter()
                            .chain(report.extra_files.iter())
                            .cloned()
                            .collect::<Vec<_>>();
                        ui.horizontal_wrapped(|ui| {
                            if report.can_restore_project_from_last_export
                                && ui.button("Update Project Save State To Match Last Exported Files").clicked()
                            {
                                self.restore_project_state_from_last_export();
                            }
                            if report.can_rewrite_files_from_project
                                && ui.button("Update Files To Match Project Save State").clicked()
                            {
                                self.write_all_generated_files_from_project_state();
                            }
                            if !importable_files.is_empty()
                                && ui.button("Semantic Import Changed Files").clicked()
                            {
                                self.import_files_semantically(&importable_files);
                            }
                            if !importable_files.is_empty()
                                && ui.button("Import Changed Files As Manual Overrides").clicked()
                            {
                                self.import_files_as_manual_overrides(&importable_files);
                            }
                            if ui.button("Continue Without Reconciling").clicked() {
                                self.show_project_sync_prompt = false;
                                self.status_line = format!(
                                    "Continuing with unresolved project sync warning: {}",
                                    report.summary
                                );
                            }
                        });
                    });
            }
        }

        egui::TopBottomPanel::top("top_bar").show(ctx, |ui| {
            self.top_bar(ui);
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.label("New Project Name");
                ui.text_edit_singleline(&mut self.new_project_name);
            });
        });

        egui::TopBottomPanel::bottom("bottom_bar").show(ctx, |ui| {
            self.bottom_bar(ui);
        });

        egui::SidePanel::left("scene_list")
            .resizable(true)
            .default_width(250.0)
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().id_salt("left_scene_panel_scroll").show(ui, |ui| {
                    self.scenes_panel(ui);
                });
            });

        egui::SidePanel::right("inspector")
            .resizable(true)
            .default_width(300.0)
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().id_salt("right_inspector_scroll").show(ui, |ui| {
                    self.inspector_panel(ui);
                });
            });

        egui::CentralPanel::default().show(ctx, |ui| {
            egui::ScrollArea::vertical().id_salt("central_workspace_scroll").show(ui, |ui| {
                self.center_panel(ui);
            });
        });

        self.floating_windows(ctx);
    }
}
