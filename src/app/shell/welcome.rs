//! The first screen: start a new game from a template, or reopen one. Each game gets its own
//! folder (by default `games/<name>` beside the engine), so the editor does not grow with them.

use std::path::{Path, PathBuf};

use eframe::egui::{self, RichText};

use super::{ForgeTheme, ProjectTemplate};
use crate::app::QuartzForgeApp;
use crate::core::project::EditorProjectState;
use crate::services::engine_location::EngineLocation;
use crate::services::persistence;

pub(in crate::app) struct WelcomeForm {
    pub name: String,
    pub location: PathBuf,
    pub template: ProjectTemplate,
    /// The New Project dialog over an open project (the welcome screen shows the form inline).
    pub dialog_open: bool,
    pub error: Option<String>,
}

impl Default for WelcomeForm {
    fn default() -> Self {
        Self {
            name: "My Game".to_owned(),
            location: EngineLocation::find().games_folder(),
            template: ProjectTemplate::Platformer,
            dialog_open: false,
            error: None,
        }
    }
}

impl WelcomeForm {
    /// The folder name for a game called `name`: lower case, words joined by underscores.
    pub fn folder_name(name: &str) -> String {
        let words: Vec<String> = name
            .split(|c: char| !c.is_alphanumeric())
            .filter(|word| !word.is_empty())
            .map(str::to_lowercase)
            .collect();
        if words.is_empty() { "game".to_owned() } else { words.join("_") }
    }

    pub fn target_folder(&self) -> PathBuf {
        self.location.join(Self::folder_name(&self.name))
    }

    fn form(&mut self, ui: &mut egui::Ui) -> bool {
        ui.horizontal(|ui| {
            ui.add_sized([72.0, 20.0], egui::Label::new(RichText::new("Name").color(ForgeTheme::MUTED)));
            ui.add(egui::TextEdit::singleline(&mut self.name).desired_width(f32::INFINITY));
        });
        ui.horizontal(|ui| {
            ui.add_sized([72.0, 20.0], egui::Label::new(RichText::new("Location").color(ForgeTheme::MUTED)));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("Change…").clicked()
                    && let Some(folder) = rfd::FileDialog::new().set_directory(&self.location).pick_folder()
                {
                    self.location = folder;
                }
                ui.add(egui::Label::new(self.location.display().to_string()).truncate()).on_hover_text(self.location.display().to_string());
            });
        });
        let target = format!("Creates {}", self.target_folder().display());
        ui.add(egui::Label::new(RichText::new(&target).small().color(ForgeTheme::MUTED)).truncate()).on_hover_text(&target);
        ui.add_space(8.0);
        ForgeTheme::caption(ui, "Template");
        let card_width = (ui.available_width() - ui.spacing().item_spacing.x) / 2.0;
        for pair in ProjectTemplate::ALL.chunks(2) {
            ui.horizontal(|ui| {
                for template in pair {
                    self.template_card(ui, *template, card_width);
                }
            });
        }
        if let Some(error) = &self.error {
            ui.colored_label(ForgeTheme::BAD, error);
        }
        ui.add_space(6.0);
        ui.horizontal(|ui| ui.add(egui::Button::new(RichText::new("Create Project").strong()).fill(ForgeTheme::ACCENT_DIM).min_size(egui::vec2(140.0, 30.0))).clicked())
            .inner
    }

    fn template_card(&mut self, ui: &mut egui::Ui, template: ProjectTemplate, width: f32) {
        let chosen = self.template == template;
        let stroke = if chosen { ForgeTheme::ACCENT } else { ForgeTheme::BORDER };
        let response = egui::Frame::new()
            .fill(if chosen { ForgeTheme::ACCENT_DIM.gamma_multiply(0.35) } else { ForgeTheme::PANEL })
            .stroke(egui::Stroke::new(1.0, stroke))
            .corner_radius(6)
            .inner_margin(egui::Margin::same(10))
            .show(ui, |ui| {
                ui.set_width(width - 22.0);
                ui.set_height(52.0);
                ui.vertical(|ui| {
                    ui.label(RichText::new(template.label()).strong());
                    ui.label(RichText::new(template.description()).small().color(ForgeTheme::MUTED));
                });
            })
            .response
            .interact(egui::Sense::click());
        if response.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
            self.template = template;
        }
    }
}

impl QuartzForgeApp {
    pub(in crate::app) fn welcome_screen(&mut self, ui: &mut egui::Ui) {
        ui.add_space((ui.available_height() * 0.08).max(16.0));
        ui.vertical_centered(|ui| {
            ui.label(RichText::new("Quartz Forge").size(30.0).strong());
            ui.label(RichText::new("Design a 2D game visually, play it live, and hand it to agents and back.").color(ForgeTheme::MUTED));
        });
        ui.add_space(24.0);
        let width = ui.available_width().min(1040.0);
        ui.horizontal(|ui| {
            ui.add_space((ui.available_width() - width).max(0.0) / 2.0);
            ui.allocate_ui(egui::vec2(width, ui.available_height()), |ui| {
                ui.columns(2, |columns| {
                    self.welcome_new_column(&mut columns[0]);
                    self.welcome_recent_column(&mut columns[1]);
                });
            });
        });
    }

    fn welcome_new_column(&mut self, ui: &mut egui::Ui) {
        Self::welcome_card(ui, |ui| {
            ui.label(RichText::new("New project").size(17.0).strong());
            ui.add_space(6.0);
            if self.shell.welcome.form(ui) {
                self.create_project_from_form();
            }
        });
    }

    fn welcome_recent_column(&mut self, ui: &mut egui::Ui) {
        Self::welcome_card(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("Recent projects").size(17.0).strong());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("Open Folder…").clicked() {
                        self.open_project_interactive();
                    }
                });
            });
            ui.add_space(6.0);
            self.recent_list(ui);
            ui.add_space(10.0);
            if ui.link("Continue without a project").clicked() {
                self.show_startup_prompt = false;
            }
        });
    }

    fn recent_list(&mut self, ui: &mut egui::Ui) {
        if self.shell.recent.entries.is_empty() {
            ui.label(RichText::new("Projects you open appear here.").color(ForgeTheme::MUTED));
            return;
        }
        let mut chosen = None;
        let mut dropped = None;
        for entry in &self.shell.recent.entries {
            let exists = entry.path.join("project.qforge.json").is_file();
            let title = if exists { RichText::new(&entry.name).strong() } else { RichText::new(format!("{} (missing)", entry.name)).color(ForgeTheme::MUTED) };
            let response = ui
                .vertical(|ui| {
                    ui.label(title);
                    ui.label(RichText::new(entry.path.display().to_string()).small().color(ForgeTheme::MUTED));
                })
                .response
                .interact(egui::Sense::click());
            let hint = if exists { "Open" } else { "The folder is gone; right-click to remove it from the list" };
            let response = response.on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text(hint);
            if response.clicked() {
                chosen = Some(entry.path.clone());
            }
            response.context_menu(|ui| {
                if ui.button("Remove from List").clicked() {
                    dropped = Some(entry.path.clone());
                }
            });
            ui.add_space(4.0);
        }
        if let Some(problem) = dropped.and_then(|path| self.shell.recent.forget(&path)) {
            self.status_line = problem;
        }
        if let Some(path) = chosen {
            self.open_project_at(&path);
        }
    }

    fn welcome_card(ui: &mut egui::Ui, content: impl FnOnce(&mut egui::Ui)) {
        egui::Frame::new()
            .fill(ForgeTheme::SURFACE)
            .stroke(egui::Stroke::new(1.0, ForgeTheme::BORDER))
            .corner_radius(8)
            .inner_margin(egui::Margin::same(18))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.set_min_height(360.0);
                content(ui);
            });
    }

    pub(in crate::app) fn new_project_dialog(&mut self, ctx: &egui::Context) {
        if !self.shell.welcome.dialog_open {
            return;
        }
        let response = egui::Modal::new(egui::Id::new("new_project_dialog")).show(ctx, |ui| {
            ui.set_width(760.0);
            ui.label(RichText::new("New project").size(17.0).strong());
            ui.add_space(6.0);
            let create = self.shell.welcome.form(ui);
            ui.add_space(4.0);
            let cancel = ui.button("Cancel").clicked();
            (create, cancel)
        });
        let (create, cancel) = response.inner;
        if create {
            self.create_project_from_form();
        }
        if cancel || response.should_close() {
            self.shell.welcome.dialog_open = false;
        }
    }

    /// Create the game folder from the form, fill it from the template, and open it.
    pub(in crate::app) fn create_project_from_form(&mut self) {
        let form = &mut self.shell.welcome;
        let name = form.name.trim().to_owned();
        if name.is_empty() {
            form.error = Some("Give the project a name.".to_owned());
            return;
        }
        let folder = form.target_folder();
        if folder.read_dir().is_ok_and(|mut entries| entries.next().is_some()) {
            form.error = Some(format!("{} already exists and is not empty.", folder.display()));
            return;
        }
        let template = form.template;
        match Self::create_from_template(&name, &folder, template) {
            Ok(state) => {
                self.shell.welcome.error = None;
                self.shell.welcome.dialog_open = false;
                self.adopt_project(state, &folder);
                self.status_line = format!("Created {name} from the {} template in {}", template.label(), folder.display());
            }
            Err(error) => self.shell.welcome.error = Some(format!("Could not create the project: {error:#}")),
        }
    }

    fn create_from_template(name: &str, folder: &Path, template: ProjectTemplate) -> anyhow::Result<EditorProjectState> {
        let mut state = persistence::create_new_project(name.to_owned(), folder)?;
        template.apply(&mut state.manifest);
        persistence::save_project(&mut state, folder)?;
        Ok(state)
    }
}
