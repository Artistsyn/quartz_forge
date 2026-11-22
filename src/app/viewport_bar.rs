//! The strip above the scene view: grid and snapping, framing the scene, and saved views.

use eframe::egui::{self, DragValue, RichText};

use super::QuartzForgeApp;
use super::shell::ForgeTheme;
use crate::core::project::SceneDocument;

const VIEW_HELP: &str = "Drag an object to move it; corner handles resize, the top handle rotates.\n\
Arrow keys nudge (Shift: 10x). Scroll to zoom, middle or right drag to pan.\n\
F frames every object.";

impl QuartzForgeApp {
    pub(super) fn viewport_bar(&mut self, ui: &mut egui::Ui, bookmarks: &[(String, String)]) {
        ui.horizontal(|ui| {
            let scene_index = self.project_state.active_scene_index;
            if let Some(scene) = self.project_state.manifest.scenes.get_mut(scene_index) {
                ui.toggle_value(&mut scene.canvas.show_grid, "Grid");
                ui.toggle_value(&mut scene.canvas.snap_to_grid, "Snap").on_hover_text("Snap moves and new objects to the grid");
            }
            ui.add(DragValue::new(&mut self.grid_size).range(8.0..=256.0).suffix(" px")).on_hover_text("Grid spacing");
            let effects_hint = self.effects.scene.failure().map_or_else(|| "Play shader and particle effects in the scene view, drawn by the engine; off shows their outlines".to_owned(), |error| format!("Effects cannot be drawn here: {error}"));
            ui.toggle_value(&mut self.effects.scene.enabled, "Show effects").on_hover_text(effects_hint);
            ui.separator();
            if ui.button("Fit").on_hover_text("Frame every object (F)").clicked() {
                self.shell.fit_requested = true;
            }
            if ui.button("Home").on_hover_text("The scene's starting view").clicked() {
                self.selected_scene_bookmark_id = "bookmark_home".to_owned();
                self.jump_to_selected_bookmark();
            }
            self.saved_views_menu(ui, bookmarks);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(RichText::new("?").strong().color(ForgeTheme::MUTED)).on_hover_text(VIEW_HELP);
            });
        });
    }

    fn saved_views_menu(&mut self, ui: &mut egui::Ui, bookmarks: &[(String, String)]) {
        let current = bookmarks.iter().find(|(id, _)| *id == self.selected_scene_bookmark_id).map_or("Saved views", |(_, name)| name.as_str());
        ui.menu_button(format!("View: {current}"), |ui| {
            for (id, name) in bookmarks {
                if ui.selectable_label(*id == self.selected_scene_bookmark_id, name).clicked() {
                    self.selected_scene_bookmark_id.clone_from(id);
                    self.jump_to_selected_bookmark();
                }
            }
            ui.separator();
            ui.add(egui::TextEdit::singleline(&mut self.new_scene_bookmark_name).hint_text("View name").desired_width(160.0));
            if ui.button("Save Current View").clicked() {
                self.add_current_view_bookmark();
            }
            if ui.add_enabled(self.selected_scene_bookmark_id != "bookmark_home", egui::Button::new("Delete This View")).clicked() {
                self.delete_selected_bookmark();
            }
        })
        .response
        .on_hover_text("Jump between saved views of the scene");
    }

    /// Zoom and pan the view so every placed object fits inside `view` (screen pixels).
    pub(super) fn fit_view(scene: &mut SceneDocument, view: egui::Vec2) {
        let bounds = Self::placed_bounds(scene);
        let canvas = &mut scene.canvas;
        let bounds = bounds.unwrap_or_else(|| egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(canvas.virtual_width, canvas.virtual_height)));
        let padded = bounds.expand(bounds.size().max_elem() * 0.06 + 16.0);
        let zoom = (view.x / padded.width()).min(view.y / padded.height()).clamp(0.1, 5.0);
        canvas.zoom = zoom;
        canvas.pan_x = padded.center().x - view.x / zoom / 2.0;
        canvas.pan_y = padded.center().y - view.y / zoom / 2.0;
    }

    /// True when the part of the scene `view` shows holds less than half of the placed objects'
    /// bounding box: a scene seen for the first time is then framed.
    pub(super) fn view_shows_little(scene: &SceneDocument, view: egui::Vec2) -> bool {
        let Some(bounds) = Self::placed_bounds(scene) else { return false };
        let canvas = &scene.canvas;
        let shown = egui::Rect::from_min_size(egui::pos2(canvas.pan_x, canvas.pan_y), view / canvas.zoom.max(0.001));
        let seen = shown.intersect(bounds);
        let seen_area = if seen.is_positive() { seen.area() } else { 0.0 };
        seen_area < bounds.area() * 0.5
    }

    fn placed_bounds(scene: &SceneDocument) -> Option<egui::Rect> {
        scene.objects.iter().filter(|blueprint| !blueprint.spawn_only).fold(None, |bounds: Option<egui::Rect>, blueprint| {
            let rect = egui::Rect::from_min_size(egui::pos2(blueprint.x, blueprint.y), egui::vec2(blueprint.w.max(1.0), blueprint.h.max(1.0)));
            Some(bounds.map_or(rect, |bounds| bounds.union(rect)))
        })
    }
}
