use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::time::Duration;

mod editors;
mod condition_editor;
mod logic_events_editor;
mod syntax_highlight;

use eframe::egui::{self, Align2, Color32, Key, Pos2, Rect, RichText, Sense, Slider, Stroke, TextEdit};
use image::AnimationDecoder;

use crate::core::project::{EditorProjectState, SceneDocument, SceneKind};
use crate::core::quartz_domain::{
    CanvasOrientation, CustomCodeKind, ObjectPhysicsMaterialPreset, ObjectTemplate,
    ObjectVisualAssetMode, SceneCanvasSpec,
};
use crate::services::codegen;
use crate::services::hot_reload::{HotReloadService, PreviewState};
use crate::services::persistence;
use crate::app::syntax_highlight::code_layouter;

#[derive(Debug, Clone, Copy)]
struct CanvasDrag {
    object_index: usize,
    start_vx: f32,
    start_vy: f32,
    start_x: f32,
    start_y: f32,
    start_w: f32,
    start_h: f32,
    resizing: bool,
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

pub struct QuartzForgeApp {
    project_root: Option<PathBuf>,
    project_state: EditorProjectState,
    hot_reload: HotReloadService,
    status_line: String,
    new_scene_name: String,
    new_project_name: String,
    selected_object_index: usize,
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
    show_constants_window: bool,
    show_game_state_window: bool,
    show_typed_vars_window: bool,
    show_custom_events_window: bool,
    show_update_loops_window: bool,
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
    selected_scene_bookmark_id: String,
    new_scene_bookmark_name: String,
    helper_var_name: String,
    helper_var_value: String,
    helper_var_type: HelperVarType,
}

impl Default for QuartzForgeApp {
    fn default() -> Self {
        Self {
            project_root: None,
            project_state: EditorProjectState::new("untitled_project".to_owned()),
            hot_reload: HotReloadService::default(),
            status_line: "Create or load a Quartz Forge project to begin.".to_owned(),
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
            show_constants_window: false,
            show_game_state_window: false,
            show_typed_vars_window: false,
            show_custom_events_window: false,
            show_update_loops_window: false,
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
            selected_scene_bookmark_id: "bookmark_home".to_owned(),
            new_scene_bookmark_name: "waypoint".to_owned(),
            helper_var_name: "score".to_owned(),
            helper_var_value: "0".to_owned(),
            helper_var_type: HelperVarType::I32,
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
        });
        ui.horizontal_wrapped(|ui| {
            ui.checkbox(&mut self.show_constants_window, "constants window");
            ui.checkbox(&mut self.show_game_state_window, "game state vars window");
            ui.checkbox(&mut self.show_typed_vars_window, "typed vars window");
            ui.checkbox(&mut self.show_custom_events_window, "custom events window");
            ui.checkbox(&mut self.show_update_loops_window, "update loops window");
            ui.checkbox(&mut self.show_top_level_window, "top level window");
            ui.checkbox(&mut self.show_file_browser_window, "file browser window");
        });

        if self.undock_scene_canvas {
            ui.label("Scene canvas is undocked to a resizable window.");
        } else {
            self.design_canvas(ui);
        }

        ui.separator();

        ui.columns(3, |cols| {
            egui::ScrollArea::vertical().id_salt("objects_editor_scroll").show(&mut cols[0], |ui| {
                self.objects_editor(ui);
            });
            egui::ScrollArea::vertical().id_salt("update_scripts_editor_scroll").show(&mut cols[1], |ui| {
                self.logic_editor(ui);
            });
            egui::ScrollArea::vertical().id_salt("events_editor_scroll").show(&mut cols[2], |ui| {
                self.events_editor(ui);
            });
        });

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
                .show(ctx, |ui| {
                    self.camera_view_panel(ui);
                });
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
        if self.show_top_level_window {
            self.custom_code_window(ctx, CustomCodeKind::TopLevel, "Top Level Code");
        }
        if self.show_file_browser_window {
            self.generated_file_browser_window(ctx);
        }
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
            ui.label("Drag objects to move. Drag corner handle to resize. Arrow keys nudge (Shift = x10). Click canvas to focus.");
        });

        let size = egui::vec2(ui.available_width(), ui.available_height().max(380.0));
        let (rect, response) = ui.allocate_exact_size(size, Sense::click_and_drag());
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 4.0, Color32::from_rgb(22, 24, 28));

        let (project_state, asset_preview_cache) =
            (&mut self.project_state, &mut self.asset_preview_cache);

        let Some(scene) = project_state
            .manifest
            .scenes
            .get_mut(project_state.active_scene_index)
        else {
            return;
        };

        let mut changed = false;

        if response.hovered() {
            let (scroll_y, pointer_pos, pointer_delta, middle_down, right_down) = ui.input(|i| {
                (
                    i.raw_scroll_delta.y,
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

            if scroll_y.abs() > f32::EPSILON {
                let pivot = pointer_pos.unwrap_or(rect.center());
                let before_z = scene.canvas.zoom.max(0.001);
                let before_x = ((pivot.x - rect.left()) / before_z) + scene.canvas.pan_x;
                let before_y = ((pivot.y - rect.top()) / before_z) + scene.canvas.pan_y;

                let factor = (1.0 + scroll_y * 0.0015).clamp(0.25, 4.0);
                scene.canvas.zoom = (scene.canvas.zoom * factor).clamp(0.1, 5.0);

                let after_z = scene.canvas.zoom.max(0.001);
                let after_x = ((pivot.x - rect.left()) / after_z) + scene.canvas.pan_x;
                let after_y = ((pivot.y - rect.top()) / after_z) + scene.canvas.pan_y;
                scene.canvas.pan_x += before_x - after_x;
                scene.canvas.pan_y += before_y - after_y;
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
            let selected = idx == self.selected_object_index;
            let fill = if selected {
                Color32::from_rgba_unmultiplied(77, 160, 255, 80)
            } else if obj.advanced.is_camera_space_pinned() {
                Color32::from_rgba_unmultiplied(120, 210, 255, 60)
            } else {
                Color32::from_rgba_unmultiplied(obj.color_rgb[0], obj.color_rgb[1], obj.color_rgb[2], 55)
            };
            let stroke = if selected {
                Stroke::new(2.0, Color32::from_rgb(77, 160, 255))
            } else if obj.lock_transform {
                Stroke::new(1.5, Color32::from_rgb(230, 150, 120))
            } else {
                Stroke::new(1.0, Color32::from_gray(180))
            };
            painter.rect_filled(obj_rect, 2.0, Color32::from_rgba_unmultiplied(255, 255, 255, 16));
            painter.rect_stroke(obj_rect, 2.0, stroke);
            match obj.template {
                ObjectTemplate::Rectangle => {
                    painter.rect_filled(obj_rect, 2.0, fill);
                }
                ObjectTemplate::Circle => {
                    let center = obj_rect.center();
                    let radius = (obj_rect.width().min(obj_rect.height()) * 0.5).max(1.0);
                    painter.circle_filled(center, radius, fill);
                    painter.circle_stroke(center, radius, stroke);
                }
            }
            let _ = Self::paint_object_asset(
                &painter,
                ui.ctx(),
                asset_preview_cache,
                project_root.as_deref(),
                obj,
                obj_rect,
            );
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
            if !obj.lock_transform {
                let handle = Rect::from_center_size(obj_rect.right_bottom(), egui::vec2(8.0, 8.0));
                painter.rect_filled(handle, 1.0, Color32::from_rgb(250, 230, 120));
            }
        }

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

        // Begin drag selection/resizing
        if response.hovered() && ui.input(|i| i.pointer.primary_pressed()) {
            if let Some(pointer) = ui.input(|i| i.pointer.interact_pos()) {
                let mut hit_index: Option<usize> = None;
                let mut resizing = false;

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
                    let handle = Rect::from_center_size(obj_rect.right_bottom(), egui::vec2(10.0, 10.0));
                    if !obj.lock_transform && handle.contains(pointer) {
                        hit_index = Some(idx);
                        resizing = true;
                        break;
                    }
                    if !obj.lock_transform && obj_rect.contains(pointer) {
                        hit_index = Some(idx);
                        resizing = false;
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
                            resizing,
                        });
                    }
                }
            }
        }

        // Continue drag
        if let Some(drag) = self.canvas_drag {
            if ui.input(|i| i.pointer.primary_down()) {
                if let Some(pointer) = ui.input(|i| i.pointer.interact_pos()) {
                    let (cvx, cvy) = to_virtual(pointer.x, pointer.y);
                    let dx = cvx - drag.start_vx;
                    let dy = cvy - drag.start_vy;
                    if drag.object_index < scene.objects.len() {
                        let obj = &mut scene.objects[drag.object_index];
                        if drag.resizing {
                            obj.w = (drag.start_w + dx).max(2.0);
                            obj.h = (drag.start_h + dy).max(2.0);
                            if scene.canvas.snap_to_grid {
                                obj.w = Self::snap_value(obj.w, self.grid_size).max(2.0);
                                obj.h = Self::snap_value(obj.h, self.grid_size).max(2.0);
                            }
                        } else {
                            obj.x = drag.start_x + dx;
                            obj.y = drag.start_y + dy;
                            Self::apply_background_snap_if_needed(obj, &scene.canvas);
                            if scene.canvas.snap_to_grid {
                                obj.x = Self::snap_value(obj.x, self.grid_size);
                                obj.y = Self::snap_value(obj.y, self.grid_size);
                            }
                        }
                        changed = true;
                    }
                }
            } else {
                self.canvas_drag = None;
            }
        }

        if changed {
            self.touch_and_refresh_preview();
        }
    }

    fn camera_view_panel(&mut self, ui: &mut egui::Ui) {
        let project_root = self.project_root.clone();
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

        let size = egui::vec2(ui.available_width(), ui.available_height().max(300.0));
        let (rect, response) = ui.allocate_exact_size(size, Sense::click_and_drag());
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 4.0, Color32::from_rgb(18, 20, 24));

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
            let rel_x = if obj.advanced.is_camera_space_pinned() { obj.x } else { obj.x - scene.canvas.camera_x };
            let rel_y = if obj.advanced.is_camera_space_pinned() { obj.y } else { obj.y - scene.canvas.camera_y };
            let p0 = Pos2::new(view_rect.left() + rel_x * scale, view_rect.top() + rel_y * scale);
            let p1 = Pos2::new(
                view_rect.left() + (rel_x + obj.w) * scale,
                view_rect.top() + (rel_y + obj.h) * scale,
            );
            let obj_rect = Rect::from_two_pos(p0, p1);
            let fill = if obj.advanced.is_camera_space_pinned() {
                Color32::from_rgba_unmultiplied(100, 220, 255, 85)
            } else {
                Color32::from_rgba_unmultiplied(obj.color_rgb[0], obj.color_rgb[1], obj.color_rgb[2], 85)
            };
            let stroke = Stroke::new(1.0, Color32::from_gray(230));
            match obj.template {
                ObjectTemplate::Rectangle => {
                    painter.rect_filled(obj_rect, 1.5, fill);
                    painter.rect_stroke(obj_rect, 1.5, stroke);
                }
                ObjectTemplate::Circle => {
                    let center = obj_rect.center();
                    let radius = (obj_rect.width().min(obj_rect.height()) * 0.5).max(1.0);
                    painter.circle_filled(center, radius, fill);
                    painter.circle_stroke(center, radius, stroke);
                }
            }
            let _ = Self::paint_object_asset(
                &painter,
                ui.ctx(),
                asset_preview_cache,
                project_root.as_deref(),
                obj,
                obj_rect,
            );
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

        if changed {
            self.touch_and_refresh_preview();
        }
    }

    fn snap_value(value: f32, grid: f32) -> f32 {
        let step = grid.max(1.0);
        (value / step).round() * step
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
        rect: Rect,
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

        painter.image(
            texture_id,
            rect,
            Rect::from_min_max(Pos2::new(0.0, 0.0), Pos2::new(1.0, 1.0)),
            Color32::WHITE,
        );
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
            out.push_str(&format!("#[path = \"{}\"]\nmod {};\n", target, module_name));
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

        out.push_str("pub fn setup_scene(canvas: &mut Canvas) {\n");
        for object in &scene.objects {
            if !object.enabled {
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
                            for (idx, snippet) in [
                                "canvas.run(Action::run_plugin(\"save_game\", \"save_slot:slot1\"));",
                                "canvas.run(Action::PluginCall {\n    name: \"terrain_collision\".to_owned(),\n    payload: std::sync::Arc::new(\"typed_payload\".to_owned()),\n});",
                                "if let Some(plugin) = canvas.get_plugin_mut::<TerrainCollisionPlugin>() {\n    // plugin.register_terrain(name, bytes, (w,h), (obj_w,obj_h), threshold, rdp);\n}",
                            ]
                            .into_iter()
                            .enumerate()
                            {
                                if ui.button(format!("insert {}", idx + 1)).clicked() {
                                    Self::append_code_block(&mut block.code, snippet);
                                    changed = true;
                                }
                                ui.label(snippet);
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

                    ui.collapsing("9) Constants Templates", |ui| {
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

                    ui.collapsing("10) Intercode Relationships", |ui| {
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
                self.helper_var_name = helper_var_name;
                self.helper_var_value = helper_var_value;
                self.helper_var_type = helper_var_type;

                if changed {
                    self.touch_and_refresh_preview();
                }
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

                let files = Self::collect_editable_files(&root);
                ui.horizontal(|ui| {
                    if ui.button("Refresh").clicked() {}
                    ui.label("Manual edits can be tracked as custom overrides.");
                });

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
        if let Some(scene) = self
            .project_state
            .manifest
            .scenes
            .get_mut(self.project_state.active_scene_index)
        {
            if let Some(existing) = scene
                .custom_code_blocks
                .iter_mut()
                .find(|b| b.kind == CustomCodeKind::ManualFileOverride && b.output_file == rel_path)
            {
                existing.code = content.to_owned();
                existing.name = format!("manual_override_{}", rel_path.replace('/', "_"));
                self.project_state.dirty = true;
                return;
            }
        }

        let (id, name) = self
            .project_state
            .manifest
            .next_custom_code_identity(CustomCodeKind::ManualFileOverride);
        let mut block = crate::core::quartz_domain::CustomCodeBlock::new(
            id,
            name,
            CustomCodeKind::ManualFileOverride,
            rel_path.to_owned(),
        );
        block.code = content.to_owned();
        if let Some(scene) = self
            .project_state
            .manifest
            .scenes
            .get_mut(self.project_state.active_scene_index)
        {
            scene.custom_code_blocks.push(block);
            self.project_state.dirty = true;
        }
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
                        format!("[{}] {} ({}){}{}{}", i + 1, o.name, o.template.as_str(), lock, cam, bg)
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

        match persistence::load_project(&path) {
            Ok(state) => {
                self.project_state = state;
                self.project_root = Some(path.clone());
                self.status_line = format!("Loaded project from {}", path.display());
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

    fn write_generated_script(&mut self) {
        let Some(root) = self.project_root.clone() else {
            self.status_line = "Open a project before writing generated script.".to_owned();
            return;
        };

        if let Err(err) = persistence::ensure_runtime_scaffold(&self.project_state, &root) {
            self.status_line = format!("Failed to prepare runtime scaffold: {err}");
            return;
        }

        if self.quartz_preview.trim().is_empty() {
            self.quartz_preview = self.build_scene_source();
        }

        let configured_rel = self
            .project_state
            .manifest
            .scenes
            .get(self.project_state.active_scene_index)
            .map(|s| s.source_file.trim().to_owned())
            .unwrap_or_default();
        let fallback_rel = format!(
            "scripts/{}",
            codegen::generated_file_name(&self.project_state)
        );
        let rel_path = if configured_rel.is_empty() {
            fallback_rel
        } else {
            configured_rel
        };

        let out_path = root.join(&rel_path);
        if let Some(parent) = out_path.parent() {
            if let Err(err) = std::fs::create_dir_all(parent) {
                self.status_line = format!("Failed to prepare output directory: {err}");
                return;
            }
        }
        self.quartz_preview = self.build_scene_source();
        let scene_override = self
            .project_state
            .manifest
            .scenes
            .get(self.project_state.active_scene_index)
            .and_then(|scene| {
                scene
                    .custom_code_blocks
                    .iter()
                    .find(|b| {
                        b.kind == CustomCodeKind::ManualFileOverride
                            && b.output_file.trim() == rel_path
                            && !b.code.trim().is_empty()
                    })
                    .map(|b| b.code.clone())
            });
        let scene_output = scene_override.unwrap_or_else(|| self.quartz_preview.clone());

        match std::fs::write(&out_path, scene_output) {
            Ok(()) => {
                self.status_line = format!("Generated Quartz script written to {}", out_path.display());
            }
            Err(err) => {
                self.status_line = format!("Failed to write generated script: {err}");
            }
        }

        if let Some(scene) = self
            .project_state
            .manifest
            .scenes
            .get(self.project_state.active_scene_index)
        {
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

                if let Some(module_source) = module_override.or_else(|| self.build_component_module_source(&target_file)) {
                    let module_path = root.join(&target_file);
                    if let Some(parent) = module_path.parent() {
                        if let Err(err) = std::fs::create_dir_all(parent) {
                            self.status_line = format!("Failed to prepare component directory: {err}");
                            continue;
                        }
                    }
                    if let Err(err) = std::fs::write(&module_path, module_source) {
                        self.status_line = format!("Failed to write component file {}: {err}", module_path.display());
                    }
                }
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
        self.quartz_preview = self.build_scene_source();
    }
}

impl eframe::App for QuartzForgeApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.hot_reload.poll();
        // Keep the editor reactive at ~60 FPS while the viewport tooling evolves.
        ctx.request_repaint_after(Duration::from_millis(16));

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
