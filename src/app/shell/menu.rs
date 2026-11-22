//! The menu bar, the toolbar under it, the status bar, and the keyboard shortcuts they list.

use eframe::egui::{self, Key, KeyboardShortcut, Modifiers, RichText};

use super::{CenterTab, CodeView, DockTab, ForgeTheme, NewObjectKind};
use crate::app::QuartzForgeApp;
use crate::services::hot_reload::PreviewState;

/// The editor's shortcuts, as the menus show them.
pub(in crate::app) struct Shortcuts;

impl Shortcuts {
    pub const NEW: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::N);
    pub const OPEN: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::O);
    pub const SAVE: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::S);
    pub const PLAY: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::R);
    pub const UNDO: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::Z);
    pub const REDO: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND.plus(Modifiers::SHIFT), Key::Z);
    pub const DUPLICATE: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::D);
    pub const DELETE: KeyboardShortcut = KeyboardShortcut::new(Modifiers::NONE, Key::Delete);
    pub const PALETTE: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::K);
}

impl QuartzForgeApp {
    pub(in crate::app) fn apply_shortcuts(&mut self, ctx: &egui::Context) {
        let typing = ctx.egui_wants_keyboard_input();
        let pressed = |shortcut: &KeyboardShortcut| ctx.input_mut(|input| input.consume_shortcut(shortcut));
        if pressed(&Shortcuts::NEW) {
            self.shell.welcome.dialog_open = true;
        }
        if pressed(&Shortcuts::OPEN) {
            self.open_project_interactive();
        }
        if pressed(&Shortcuts::SAVE) {
            self.save_project();
        }
        if pressed(&Shortcuts::PLAY) {
            self.toggle_live_play();
        }
        if pressed(&Shortcuts::PALETTE) {
            self.shell.palette.open();
        }
        if typing {
            return;
        }
        if pressed(&Shortcuts::REDO) {
            self.redo();
        } else if pressed(&Shortcuts::UNDO) {
            self.undo();
        }
        if ctx.input_mut(|input| input.consume_key(Modifiers::NONE, Key::F)) {
            self.shell.fit_requested = true;
        }
        if pressed(&Shortcuts::DUPLICATE) {
            self.duplicate_selected_object();
        }
        if pressed(&Shortcuts::DELETE) || ctx.input_mut(|input| input.consume_key(Modifiers::NONE, Key::Backspace)) {
            self.delete_selected_object();
        }
    }

    pub(in crate::app) fn menu_bar(&mut self, ui: &mut egui::Ui) {
        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button("File", |ui| self.file_menu(ui));
            let open = self.project_root.is_some();
            ui.add_enabled_ui(open, |ui| {
                ui.menu_button("Edit", |ui| self.edit_menu(ui));
                ui.menu_button("View", |ui| self.view_menu(ui));
                ui.menu_button("Scene", |ui| self.scene_menu(ui));
                ui.menu_button("Play", |ui| self.play_menu(ui));
            });
            ui.menu_button("Tools", |ui| self.tools_menu(ui));
            ui.menu_button("Help", |ui| self.help_menu(ui));
        });
    }

    fn shortcut_button(ui: &mut egui::Ui, text: &str, shortcut: &KeyboardShortcut) -> bool {
        let hint = ui.ctx().format_shortcut(shortcut);
        ui.add(egui::Button::new(text).shortcut_text(hint)).clicked()
    }

    fn file_menu(&mut self, ui: &mut egui::Ui) {
        if Self::shortcut_button(ui, "New Project…", &Shortcuts::NEW) {
            self.shell.welcome.dialog_open = true;
        }
        if Self::shortcut_button(ui, "Open Project…", &Shortcuts::OPEN) {
            self.open_project_interactive();
        }
        ui.menu_button("Open Recent", |ui| {
            let entries = self.shell.recent.entries.clone();
            if entries.is_empty() {
                ui.label(RichText::new("No recent projects").color(ForgeTheme::MUTED));
            }
            for entry in entries {
                if ui.button(&entry.name).on_hover_text(entry.path.display().to_string()).clicked() {
                    self.open_project_at(&entry.path);
                }
            }
        });
        ui.separator();
        if ui.add_enabled(self.project_root.is_some(), egui::Button::new("Save").shortcut_text(ui.ctx().format_shortcut(&Shortcuts::SAVE))).clicked() {
            self.save_project();
        }
        if ui.add_enabled(self.project_root.is_some(), egui::Button::new("Close Project")).clicked() {
            self.close_project();
        }
        ui.separator();
        if ui.button("Quit").clicked() {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }

    fn edit_menu(&mut self, ui: &mut egui::Ui) {
        if ui.add_enabled(self.shell.history.can_undo(), egui::Button::new("Undo").shortcut_text(ui.ctx().format_shortcut(&Shortcuts::UNDO))).clicked() {
            self.undo();
        }
        if ui.add_enabled(self.shell.history.can_redo(), egui::Button::new("Redo").shortcut_text(ui.ctx().format_shortcut(&Shortcuts::REDO))).clicked() {
            self.redo();
        }
        ui.separator();
        for kind in NewObjectKind::ALL {
            if ui.button(kind.menu_label()).clicked() {
                self.add_object_of(kind);
            }
        }
        ui.separator();
        if Self::shortcut_button(ui, "Duplicate Object", &Shortcuts::DUPLICATE) {
            self.duplicate_selected_object();
        }
        if Self::shortcut_button(ui, "Delete Object", &Shortcuts::DELETE) {
            self.delete_selected_object();
        }
        ui.separator();
        if Self::shortcut_button(ui, "Command Palette…", &Shortcuts::PALETTE) {
            self.shell.palette.open();
        }
    }

    fn view_menu(&mut self, ui: &mut egui::Ui) {
        ui.radio_value(&mut self.shell.tabs.center, CenterTab::Scene, "Scene View");
        ui.radio_value(&mut self.shell.tabs.center, CenterTab::Camera, "Camera View");
        ui.radio_value(&mut self.shell.tabs.center, CenterTab::Code, "Generated Code");
        ui.separator();
        ui.checkbox(&mut self.shell.tabs.dock_open, "Bottom Panel");
        ui.checkbox(&mut self.show_pivot_points, "Pivot Points");
        ui.checkbox(&mut self.show_spawn_overlay, "Spawn Overlay");
        ui.checkbox(&mut self.show_camera_view_grid, "Camera View Grid");
        ui.separator();
        ui.menu_button("Floating Windows", |ui| {
            ui.checkbox(&mut self.show_object_menu_window, "Object Builder");
            ui.checkbox(&mut self.show_event_builder_window, "Event Builder");
            ui.checkbox(&mut self.show_camera_view_window, "Camera View");
            ui.checkbox(&mut self.undock_scene_canvas, "Scene Canvas");
        });
    }

    fn scene_menu(&mut self, ui: &mut egui::Ui) {
        if ui.button("New Scene").clicked() {
            self.add_scene_named_next();
        }
        if ui.add_enabled(self.project_state.manifest.scenes.len() > 1, egui::Button::new("Delete Scene")).clicked() {
            self.delete_active_scene();
        }
        if ui.button("Scene Settings").clicked() {
            self.shell.tabs.inspector = super::InspectorTab::Scene;
        }
        ui.separator();
        let names: Vec<String> = self.project_state.manifest.scenes.iter().map(|scene| scene.name.clone()).collect();
        for (index, name) in names.iter().enumerate() {
            if ui.radio(index == self.project_state.active_scene_index, name).clicked() {
                self.select_scene(index);
            }
        }
    }

    fn play_menu(&mut self, ui: &mut egui::Ui) {
        let live = if self.live_play.is_running() { "Stop Live Play" } else { "Play Live" };
        if Self::shortcut_button(ui, live, &Shortcuts::PLAY) {
            self.toggle_live_play();
        }
        ui.separator();
        let built = matches!(self.hot_reload.state, PreviewState::Running);
        if ui.add_enabled(!built, egui::Button::new("Build and Run")).on_hover_text("Generate the Rust code, compile it and run it").clicked() {
            self.start_preview();
        }
        if ui.add_enabled(built, egui::Button::new("Stop Build")).clicked() {
            self.stop_preview();
        }
    }

    fn tools_menu(&mut self, ui: &mut egui::Ui) {
        let waiting = self.shell.gaps.waiting();
        let label = if waiting > 0 { format!("Gap Reports ({waiting} waiting)") } else { "Gap Reports".to_owned() };
        if ui.button(label).clicked() {
            self.shell.gaps.show();
        }
        ui.separator();
        ui.add_enabled_ui(self.project_root.is_some(), |ui| self.project_tools(ui));
    }

    fn project_tools(&mut self, ui: &mut egui::Ui) {
        ui.menu_button("Code Blocks", |ui| {
            for view in CodeView::ALL {
                if ui.button(view.label()).clicked() {
                    self.shell.tabs.code_view = view;
                    self.show_dock_tab(DockTab::CodeBlocks);
                }
            }
        });
        ui.checkbox(&mut self.show_background_designer_window, "Background Designer");
        ui.checkbox(&mut self.show_grapple_wizard_window, "Grapple Wizard");
        ui.separator();
        ui.menu_button("Advanced", |ui| {
            if ui.button("Regenerate Code Preview").clicked() {
                self.quartz_preview = self.build_scene_source();
                self.show_center_tab(CenterTab::Code);
            }
            if ui.button("Write Generated Code").clicked() {
                self.write_generated_script();
            }
            if ui.button("Check Round Trip").on_hover_text("Generate the code and import it back, reporting anything that would change").clicked() {
                self.pre_generate_roundtrip_check();
            }
            if ui.button("Import Edited Code").on_hover_text("Read hand edits in the generated Rust back into the project").clicked() {
                self.manual_semantic_reimport();
            }
        });
    }

    fn help_menu(&mut self, ui: &mut egui::Ui) {
        let manual = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("USER_MANUAL.md");
        if ui.button("User Manual").clicked() {
            ui.ctx().open_url(egui::OpenUrl::new_tab(format!("file://{}", manual.display())));
        }
        if ui.button("Keyboard Shortcuts").clicked() {
            self.shell.show_shortcuts = true;
        }
        ui.separator();
        ui.label(RichText::new(format!("Quartz Forge {}", env!("CARGO_PKG_VERSION"))).color(ForgeTheme::MUTED));
    }

    pub(in crate::app) fn toolbar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.add_space(2.0);
            let dirty = if self.project_state.dirty { " •" } else { "" };
            ui.label(RichText::new(format!("{}{dirty}", self.project_state.manifest.project_name)).strong().size(14.5));
            ui.separator();
            self.scene_picker(ui);
            ui.separator();
            if ui.button("Save").on_hover_text(ui.ctx().format_shortcut(&Shortcuts::SAVE)).clicked() {
                self.save_project();
            }
            if ui.add_enabled(self.shell.history.can_undo(), egui::Button::new("⟲")).on_hover_text("Undo").clicked() {
                self.undo();
            }
            if ui.add_enabled(self.shell.history.can_redo(), egui::Button::new("⟳")).on_hover_text("Redo").clicked() {
                self.redo();
            }
            ui.separator();
            self.play_controls(ui);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| self.toolbar_right(ui));
        });
    }

    fn scene_picker(&mut self, ui: &mut egui::Ui) {
        let active = self.project_state.active_scene_index;
        let names: Vec<String> = self.project_state.manifest.scenes.iter().map(|scene| scene.name.clone()).collect();
        let current = names.get(active).cloned().unwrap_or_default();
        egui::ComboBox::from_id_salt("toolbar_scene").selected_text(current).width(160.0).show_ui(ui, |ui| {
            for (index, name) in names.iter().enumerate() {
                if ui.selectable_label(index == active, name).clicked() {
                    self.select_scene(index);
                }
            }
        });
    }

    fn play_controls(&mut self, ui: &mut egui::Ui) {
        let running = self.live_play.is_running();
        let (label, fill) = if running { ("⏹  Stop", ForgeTheme::BAD.gamma_multiply(0.55)) } else { ("▶  Play Live", ForgeTheme::GOOD.gamma_multiply(0.45)) };
        let hint = format!("Run the game from its data; edits show while it plays ({})", ui.ctx().format_shortcut(&Shortcuts::PLAY));
        if ui.add(egui::Button::new(RichText::new(label).strong().color(egui::Color32::WHITE)).fill(fill)).on_hover_text(hint).clicked() {
            self.toggle_live_play();
        }
    }

    fn toolbar_right(&mut self, ui: &mut egui::Ui) {
        let waiting = self.shell.gaps.waiting();
        if waiting > 0
            && ui.add(egui::Button::new(RichText::new(format!("{waiting} gap report{}", if waiting == 1 { "" } else { "s" })).color(ForgeTheme::WARN)))
                .on_hover_text("Agents found something a game needs; review their proposals")
                .clicked()
        {
            self.shell.gaps.show();
        }
        let (text, color) = match self.hot_reload.state {
            PreviewState::Running => ("Build running", ForgeTheme::GOOD),
            PreviewState::Failed => ("Build failed", ForgeTheme::BAD),
            PreviewState::Exited | PreviewState::Stopped => ("", ForgeTheme::MUTED),
        };
        if !text.is_empty() {
            ForgeTheme::chip(ui, text, color);
        }
        if self.live_play.is_running() {
            ForgeTheme::chip(ui, "Live", ForgeTheme::GOOD);
        }
    }

    pub(in crate::app) fn status_bar(&mut self, ui: &mut egui::Ui) {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if let Some(root) = &self.project_root {
                ui.add(egui::Label::new(RichText::new(root.display().to_string()).small().color(ForgeTheme::MUTED)).truncate())
                    .on_hover_text(root.display().to_string());
                ui.separator();
            }
            ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                ui.add(egui::Label::new(RichText::new(&self.status_line).small()).truncate()).on_hover_text(&self.status_line);
            });
        });
    }

    pub(in crate::app) fn shortcuts_window(&mut self, ctx: &egui::Context) {
        let rows = [
            ("New project", Shortcuts::NEW),
            ("Open project", Shortcuts::OPEN),
            ("Save", Shortcuts::SAVE),
            ("Play live / stop", Shortcuts::PLAY),
            ("Undo", Shortcuts::UNDO),
            ("Redo", Shortcuts::REDO),
            ("Duplicate object", Shortcuts::DUPLICATE),
            ("Delete object", Shortcuts::DELETE),
            ("Command palette", Shortcuts::PALETTE),
            ("Frame every object", KeyboardShortcut::new(Modifiers::NONE, Key::F)),
        ];
        let mut open = self.shell.show_shortcuts;
        egui::Window::new("Keyboard Shortcuts").open(&mut open).collapsible(false).resizable(false).show(ctx, |ui| {
            egui::Grid::new("shortcut_grid").num_columns(2).spacing([24.0, 6.0]).show(ui, |ui| {
                for (what, shortcut) in rows {
                    ui.label(what);
                    ui.label(RichText::new(ctx.format_shortcut(&shortcut)).monospace().color(ForgeTheme::MUTED));
                    ui.end_row();
                }
                ui.label("Nudge selected object");
                ui.label(RichText::new("Arrow keys in the scene view").color(ForgeTheme::MUTED));
                ui.end_row();
            });
        });
        self.shell.show_shortcuts = open;
    }
}
