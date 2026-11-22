//! The docked panels: scenes and the object hierarchy on the left, the inspector on the right,
//! the scene view in the centre, and the bottom panel with events, code, background, lighting,
//! files and the output log.

use eframe::egui::{self, RichText};

use super::{CenterTab, CodeView, DockTab, ForgeTheme, InspectorTab, NewObjectKind};
use crate::app::QuartzForgeApp;
use crate::core::quartz_domain::{ObjectTemplate, QuartzObjectBlueprint};

impl QuartzForgeApp {
    pub(in crate::app) fn outliner(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ForgeTheme::caption(ui, "Scenes");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.small_button("+").on_hover_text("New scene").clicked() {
                    self.add_scene_named_next();
                }
            });
        });
        self.scene_rows(ui);
        ui.add_space(6.0);
        ui.separator();
        ui.horizontal(|ui| {
            ForgeTheme::caption(ui, "Hierarchy");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| self.add_object_menu(ui));
        });
        ui.add(egui::TextEdit::singleline(&mut self.shell.hierarchy_filter).hint_text("Filter objects").desired_width(f32::INFINITY));
        egui::ScrollArea::vertical().id_salt("hierarchy_scroll").auto_shrink([false, false]).show(ui, |ui| self.object_rows(ui));
    }

    fn add_object_menu(&mut self, ui: &mut egui::Ui) {
        ui.menu_button("+", |ui| {
            for kind in NewObjectKind::ALL {
                if ui.button(kind.menu_label()).on_hover_text(kind.hint()).clicked() {
                    self.add_object_of(kind);
                }
            }
        })
        .response
        .on_hover_text("Add an object");
    }

    fn scene_rows(&mut self, ui: &mut egui::Ui) {
        let names: Vec<(String, usize)> =
            self.project_state.manifest.scenes.iter().map(|scene| (scene.name.clone(), scene.objects.len())).collect();
        let can_delete = names.len() > 1;
        for (index, (name, count)) in names.iter().enumerate() {
            let active = index == self.project_state.active_scene_index;
            let response = ui.add(egui::Button::selectable(active, name.as_str()).right_text(RichText::new(count.to_string()).small().color(ForgeTheme::MUTED)).min_size(egui::vec2(ui.available_width(), 0.0)));
            if response.clicked() {
                self.select_scene(index);
            }
            response.context_menu(|ui| {
                if ui.button("Scene Settings").clicked() {
                    self.select_scene(index);
                    self.shell.tabs.inspector = InspectorTab::Scene;
                }
                if ui.add_enabled(can_delete, egui::Button::new("Delete Scene")).clicked() {
                    self.select_scene(index);
                    self.delete_active_scene();
                }
            });
        }
    }

    fn object_rows(&mut self, ui: &mut egui::Ui) {
        let filter = self.shell.hierarchy_filter.to_lowercase();
        let rows: Vec<(usize, String, String, String)> = self
            .project_state
            .manifest
            .scenes
            .get(self.project_state.active_scene_index)
            .map(|scene| {
                scene
                    .objects
                    .iter()
                    .enumerate()
                    .filter(|(_, blueprint)| filter.is_empty() || blueprint.name.to_lowercase().contains(&filter) || blueprint.tags.iter().any(|tag| tag.contains(&filter)))
                    .map(|(index, blueprint)| (index, Self::object_glyph(blueprint).to_owned(), blueprint.name.clone(), Self::object_note(blueprint)))
                    .collect()
            })
            .unwrap_or_default();
        if rows.is_empty() {
            let hint = if filter.is_empty() { "No objects yet. Use + to add one." } else { "No object matches." };
            ui.label(RichText::new(hint).color(ForgeTheme::MUTED));
        }
        for (index, glyph, name, note) in rows {
            let selected = index == self.selected_object_index;
            let label = RichText::new(format!("{glyph}  {name}"));
            let response = ui.add(egui::Button::selectable(selected, label).right_text(RichText::new(note).small().color(ForgeTheme::MUTED)).min_size(egui::vec2(ui.available_width(), 0.0)));
            if response.clicked() {
                self.select_object(index);
            }
            response.context_menu(|ui| self.object_context_menu(ui, index));
        }
    }

    fn object_context_menu(&mut self, ui: &mut egui::Ui, index: usize) {
        self.select_object(index);
        if ui.button("Duplicate").clicked() {
            self.duplicate_selected_object();
        }
        if ui.button("Delete").clicked() {
            self.delete_selected_object();
        }
        let scene_index = self.project_state.active_scene_index;
        if let Some(blueprint) = self.project_state.manifest.scenes.get_mut(scene_index).and_then(|scene| scene.objects.get_mut(index)) {
            let label = if blueprint.lock_transform { "Unlock Position" } else { "Lock Position" };
            if ui.button(label).clicked() {
                blueprint.lock_transform = !blueprint.lock_transform;
                self.touch_and_refresh_preview();
            }
        }
    }

    fn object_glyph(blueprint: &QuartzObjectBlueprint) -> &'static str {
        if blueprint.spawn_only {
            "○"
        } else if blueprint.is_background {
            "▣"
        } else if blueprint.template == ObjectTemplate::Circle {
            "●"
        } else {
            "■"
        }
    }

    fn object_note(blueprint: &QuartzObjectBlueprint) -> String {
        let mut notes: Vec<&str> = Vec::new();
        if blueprint.spawn_only {
            notes.push("template");
        }
        if blueprint.lock_transform {
            notes.push("locked");
        }
        if blueprint.advanced.is_camera_space_pinned() {
            notes.push("HUD");
        }
        notes.extend(blueprint.tags.iter().take(2).map(String::as_str));
        notes.join(" · ")
    }

    pub(in crate::app) fn inspector(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.shell.tabs.inspector, InspectorTab::Object, "Object");
            ui.selectable_value(&mut self.shell.tabs.inspector, InspectorTab::Scene, "Scene Settings");
        });
        ui.separator();
        egui::ScrollArea::vertical().id_salt("inspector_scroll").auto_shrink([false, false]).show(ui, |ui| match self.shell.tabs.inspector {
            InspectorTab::Object => self.object_inspector(ui),
            InspectorTab::Scene => self.inspector_panel(ui),
        });
    }

    fn object_inspector(&mut self, ui: &mut egui::Ui) {
        let has_object = self
            .project_state
            .manifest
            .scenes
            .get(self.project_state.active_scene_index)
            .is_some_and(|scene| self.selected_object_index < scene.objects.len());
        if has_object {
            self.object_details(ui);
        } else {
            ui.add_space(12.0);
            ui.label(RichText::new("Select an object in the hierarchy or the scene view.").color(ForgeTheme::MUTED));
        }
    }

    pub(in crate::app) fn center(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            for tab in CenterTab::ALL {
                if ui.selectable_label(self.shell.tabs.center == tab, tab.label()).clicked() {
                    self.show_center_tab(tab);
                }
            }
            if self.shell.tabs.center == CenterTab::Scene {
                ui.separator();
                ui.toggle_value(&mut self.show_pivot_points, "Pivots").on_hover_text("Show each object's rotation pivot");
                ui.toggle_value(&mut self.show_spawn_overlay, "Spawns").on_hover_text("Show where spawn actions place objects");
            }
        });
        ui.separator();
        match self.shell.tabs.center {
            CenterTab::Scene => self.design_canvas(ui),
            CenterTab::Camera => self.camera_view_panel(ui),
            CenterTab::Code => self.code_view(ui),
        }
    }

    fn code_view(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(RichText::new("The Rust this scene generates (read-only here; edit the scene or a code block).").color(ForgeTheme::MUTED));
            if ui.button("Refresh").clicked() {
                self.quartz_preview = self.build_scene_source();
            }
        });
        egui::ScrollArea::both().id_salt("code_view_scroll").auto_shrink([false, false]).show(ui, |ui| {
            let mut text = self.quartz_preview.as_str();
            ui.add(egui::TextEdit::multiline(&mut text).code_editor().desired_width(f32::INFINITY));
        });
    }

    pub(in crate::app) fn dock_panel(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            for tab in DockTab::ALL {
                ui.selectable_value(&mut self.shell.tabs.dock, tab, tab.label());
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.small_button("⬇").on_hover_text("Hide the bottom panel").clicked() {
                    self.shell.tabs.dock_open = false;
                }
            });
        });
        ui.separator();
        if self.shell.tabs.dock == DockTab::Effects {
            self.effects_tab(ui);
            return;
        }
        egui::ScrollArea::vertical().id_salt(("dock_scroll", self.shell.tabs.dock)).auto_shrink([false, false]).show(ui, |ui| match self.shell.tabs.dock {
            DockTab::Events => self.rules_editor(ui),
            DockTab::CodeBlocks => self.code_blocks(ui),
            DockTab::Background => self.background_window_panel(ui),
            DockTab::Lighting => self.lighting_window_panel(ui),
            DockTab::Effects => self.effects_tab(ui),
            DockTab::Files => self.file_browser_body(ui),
            DockTab::Output => self.output_log(ui),
        });
    }

    fn code_blocks(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            for view in CodeView::ALL {
                ui.selectable_value(&mut self.shell.tabs.code_view, view, view.label());
            }
        });
        ui.separator();
        match self.shell.tabs.code_view {
            CodeView::Kind(kind, label) => self.custom_code_body(ui, kind, label),
            CodeView::Other => self.custom_code_other_body(ui),
        }
    }

    fn output_log(&mut self, ui: &mut egui::Ui) {
        if self.shell.output.lines.is_empty() {
            ui.label(RichText::new("Messages from saving, playing and building appear here.").color(ForgeTheme::MUTED));
        }
        for (time, line) in self.shell.output.lines.iter().rev() {
            ui.horizontal(|ui| {
                ui.label(RichText::new(time).monospace().small().color(ForgeTheme::MUTED));
                ui.label(line);
            });
        }
    }
}
