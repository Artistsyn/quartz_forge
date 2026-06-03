use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::time::Duration;

mod editors;
mod condition_editor;
mod logic_events_editor;

use eframe::egui::{self, Align2, Color32, Key, Pos2, Rect, RichText, Sense, Slider, Stroke, TextEdit};
use image::AnimationDecoder;

use crate::core::project::{EditorProjectState, SceneKind};
use crate::core::quartz_domain::{
    ObjectPhysicsMaterialPreset, ObjectTemplate, ObjectVisualAssetMode,
};
use crate::services::codegen;
use crate::services::hot_reload::{HotReloadService, PreviewState};
use crate::services::persistence;

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
    show_startup_prompt: bool,
    camera_view_drag: Option<CameraViewDrag>,
    asset_preview_cache: HashMap<String, AssetPreviewTextures>,
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
            show_startup_prompt: true,
            camera_view_drag: None,
            asset_preview_cache: HashMap::new(),
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
        ui.add(Slider::new(&mut scene.canvas.virtual_width, 320.0..=7680.0).text("virtual width"));
        ui.add(Slider::new(&mut scene.canvas.virtual_height, 180.0..=4320.0).text("virtual height"));
        ui.add(Slider::new(&mut scene.canvas.zoom, 0.1..=5.0).text("zoom"));
        ui.add(Slider::new(&mut scene.canvas.pan_x, -5000.0..=5000.0).text("pan x"));
        ui.add(Slider::new(&mut scene.canvas.pan_y, -5000.0..=5000.0).text("pan y"));
        ui.checkbox(&mut scene.canvas.snap_to_grid, "snap object edits to grid");
        ui.checkbox(&mut scene.canvas.show_camera_frame, "show camera frame overlay");
        ui.checkbox(&mut scene.canvas.show_grid, "show scene grid");
        ui.add(Slider::new(&mut scene.canvas.camera_x, -8000.0..=8000.0).text("camera x"));
        ui.add(Slider::new(&mut scene.canvas.camera_y, -8000.0..=8000.0).text("camera y"));
        ui.add(Slider::new(&mut scene.canvas.camera_width, 64.0..=7680.0).text("camera width"));
        ui.add(Slider::new(&mut scene.canvas.camera_height, 64.0..=4320.0).text("camera height"));
        ui.label("Probe Screen -> Virtual roundtrip");
        ui.add(Slider::new(&mut self.probe_screen_x, -2000.0..=4000.0).text("screen x"));
        ui.add(Slider::new(&mut self.probe_screen_y, -2000.0..=3000.0).text("screen y"));
        let (vx, vy) = scene.canvas.screen_to_virtual(self.probe_screen_x, self.probe_screen_y);
        let (sx2, sy2) = scene.canvas.virtual_to_screen(vx, vy);
        ui.label(format!("virtual: ({vx:.2}, {vy:.2})"));
        ui.label(format!("roundtrip: ({sx2:.2}, {sy2:.2})"));

        ui.separator();
        ui.label("Quartz Forge Contract");
        ui.label("- Scene files live in /scenes");
        ui.label("- Logic scripts live in /scripts");
        ui.label("- Asset roots live in /assets/*");
        ui.label("- Preview runner targets the project root crate");
    }

    fn center_panel(&mut self, ui: &mut egui::Ui) {
        ui.heading("Quartz Authoring Workspace");
        ui.add_space(8.0);

        ui.horizontal(|ui| {
            ui.checkbox(&mut self.undock_scene_canvas, "undock scene canvas window");
            ui.checkbox(&mut self.show_camera_view_window, "show camera view window");
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
    }

    fn design_canvas(&mut self, ui: &mut egui::Ui) {
        let project_root = self.project_root.clone();
        let (project_state, asset_preview_cache) =
            (&mut self.project_state, &mut self.asset_preview_cache);
        ui.horizontal(|ui| {
            ui.label("Visual Scene Canvas");
            ui.add(Slider::new(&mut self.grid_size, 8.0..=256.0).text("grid"));
            ui.label("Drag objects to move. Drag corner handle to resize. Arrow keys nudge (Shift = x10). Click canvas to focus.");
        });

        let size = egui::vec2(ui.available_width(), ui.available_height().max(380.0));
        let (rect, response) = ui.allocate_exact_size(size, Sense::click_and_drag());
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 4.0, Color32::from_rgb(22, 24, 28));

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

        if scene.canvas.show_grid {
            let minor = Stroke::new(1.0, Color32::from_gray(46));
            let major = Stroke::new(1.0, Color32::from_gray(74));
            let mut x = rect.left();
            let mut column = 0usize;
            while x <= rect.right() {
                let stroke = if column % 4 == 0 { major } else { minor };
                painter.line_segment([Pos2::new(x, rect.top()), Pos2::new(x, rect.bottom())], stroke);
                x += self.grid_size;
                column += 1;
            }
            let mut y = rect.top();
            let mut row = 0usize;
            while y <= rect.bottom() {
                let stroke = if row % 4 == 0 { major } else { minor };
                painter.line_segment([Pos2::new(rect.left(), y), Pos2::new(rect.right(), y)], stroke);
                y += self.grid_size;
                row += 1;
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
                        if let Ok(rel) = path.strip_prefix(root) {
                            selected = rel.to_string_lossy().replace('\\', "/");
                        } else {
                            selected = path.to_string_lossy().replace('\\', "/");
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

        let mut out = String::new();
        out.push_str("use quartz::prelude::*;\n");
        for (target, module_name) in &external_modules {
            out.push_str(&format!("#[path = \"{}\"]\nmod {};\n", target, module_name));
            out.push_str(&format!("use {}::*;\n", module_name));
        }
        if !external_modules.is_empty() {
            out.push_str("\n");
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
        out.push_str("}\n\n");

        out.push_str("pub fn register_logic(canvas: &mut Canvas) {\n");
        for tree in &scene.logic_trees {
            out.push_str(&format!("    // Update Script: {}\n", tree.name));
            out.push_str("    canvas.on_update(|canvas| {\n");
            out.push_str(&format!("        canvas.run({});\n", codegen::logic_tree_action_expr(tree)));
            out.push_str("    });\n");
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

        if wrote_any {
            Some(out)
        } else {
            None
        }
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
                        format!("[{}] {} ({}){}{}", i + 1, o.name, o.template.as_str(), lock, cam)
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

        match std::fs::write(&out_path, &self.quartz_preview) {
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

            for target_file in target_files {
                if let Some(module_source) = self.build_component_module_source(&target_file) {
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
