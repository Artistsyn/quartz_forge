//! The command palette (Cmd+K): type a few letters of any command, scene or object and press
//! Enter.

use eframe::egui::{self, Key, RichText};

use super::{CenterTab, DockTab, ForgeTheme, NewObjectKind};
use crate::app::QuartzForgeApp;

#[derive(Debug, Clone, PartialEq)]
enum Command {
    NewProject,
    OpenProject,
    Save,
    PlayLive,
    BuildAndRun,
    Undo,
    Redo,
    AddObject(NewObjectKind),
    Duplicate,
    Delete,
    NewScene,
    Center(CenterTab),
    Dock(DockTab),
    GapReports,
    Shortcuts,
    GoToScene(usize),
    SelectObject(usize),
}

#[derive(Debug, Default)]
pub(in crate::app) struct CommandPalette {
    visible: bool,
    query: String,
    highlighted: usize,
}

impl CommandPalette {
    pub fn open(&mut self) {
        self.visible = true;
        self.query.clear();
        self.highlighted = 0;
    }

    /// True when every character of `query` appears in `label` in order (case ignored).
    fn matches(label: &str, query: &str) -> bool {
        let mut letters = label.chars().flat_map(char::to_lowercase);
        query.chars().flat_map(char::to_lowercase).filter(|c| !c.is_whitespace()).all(|wanted| letters.any(|c| c == wanted))
    }
}

impl QuartzForgeApp {
    fn palette_commands(&mut self) -> Vec<(String, Command)> {
        let mut commands: Vec<(String, Command)> = vec![
            ("New Project…".to_owned(), Command::NewProject),
            ("Open Project…".to_owned(), Command::OpenProject),
            ("Save".to_owned(), Command::Save),
            ((if self.live_play.is_running() { "Stop Live Play" } else { "Play Live" }).to_owned(), Command::PlayLive),
            ("Build and Run".to_owned(), Command::BuildAndRun),
            ("Undo".to_owned(), Command::Undo),
            ("Redo".to_owned(), Command::Redo),
            ("Duplicate Object".to_owned(), Command::Duplicate),
            ("Delete Object".to_owned(), Command::Delete),
            ("New Scene".to_owned(), Command::NewScene),
            ("Gap Reports".to_owned(), Command::GapReports),
            ("Keyboard Shortcuts".to_owned(), Command::Shortcuts),
        ];
        commands.extend(NewObjectKind::ALL.map(|kind| (kind.menu_label().to_owned(), Command::AddObject(kind))));
        commands.extend(CenterTab::ALL.map(|tab| (format!("Show {}", tab.label()), Command::Center(tab))));
        commands.extend(DockTab::ALL.map(|tab| (format!("Show {}", tab.label()), Command::Dock(tab))));
        let scenes = &self.project_state.manifest.scenes;
        commands.extend(scenes.iter().enumerate().map(|(index, scene)| (format!("Go to scene: {}", scene.name), Command::GoToScene(index))));
        if let Some(scene) = scenes.get(self.project_state.active_scene_index) {
            commands.extend(scene.objects.iter().enumerate().map(|(index, blueprint)| (format!("Select: {}", blueprint.name),Command::SelectObject(index))));
        }
        commands
    }

    fn run_command(&mut self, command: Command) {
        match command {
            Command::NewProject => self.shell.welcome.dialog_open = true,
            Command::OpenProject => self.open_project_interactive(),
            Command::Save => self.save_project(),
            Command::PlayLive => self.toggle_live_play(),
            Command::BuildAndRun => self.start_preview(),
            Command::Undo => self.undo(),
            Command::Redo => self.redo(),
            Command::AddObject(kind) => self.add_object_of(kind),
            Command::Duplicate => self.duplicate_selected_object(),
            Command::Delete => self.delete_selected_object(),
            Command::NewScene => self.add_scene_named_next(),
            Command::Center(tab) => self.show_center_tab(tab),
            Command::Dock(tab) => self.show_dock_tab(tab),
            Command::GapReports => self.shell.gaps.show(),
            Command::Shortcuts => self.shell.show_shortcuts = true,
            Command::GoToScene(index) => self.select_scene(index),
            Command::SelectObject(index) => self.select_object(index),
        }
    }

    pub(in crate::app) fn command_palette(&mut self, ctx: &egui::Context) {
        if !self.shell.palette.visible {
            return;
        }
        let query = self.shell.palette.query.clone();
        let shown: Vec<(String, Command)> =
            self.palette_commands().into_iter().filter(|(label, _)| CommandPalette::matches(label, &query)).collect();
        let (enter, escape, down, up) = ctx.input(|input| {
            (input.key_pressed(Key::Enter), input.key_pressed(Key::Escape), input.key_pressed(Key::ArrowDown), input.key_pressed(Key::ArrowUp))
        });
        let palette = &mut self.shell.palette;
        palette.highlighted = if down { palette.highlighted + 1 } else if up { palette.highlighted.saturating_sub(1) } else { palette.highlighted };
        palette.highlighted = palette.highlighted.min(shown.len().saturating_sub(1));
        let mut chosen = enter.then(|| shown.get(palette.highlighted).map(|(_, command)| command.clone())).flatten();
        let response = egui::Modal::new(egui::Id::new("command_palette")).show(ctx, |ui| {
            ui.set_width(520.0);
            let edit = ui.add(egui::TextEdit::singleline(&mut palette.query).hint_text("Type a command, scene or object").desired_width(f32::INFINITY));
            edit.request_focus();
            ui.separator();
            egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
                for (index, (label, command)) in shown.iter().enumerate() {
                    if ui.selectable_label(index == palette.highlighted, label).clicked() {
                        chosen = Some(command.clone());
                    }
                }
                if shown.is_empty() {
                    ui.label(RichText::new("Nothing matches.").color(ForgeTheme::MUTED));
                }
            });
        });
        if escape || chosen.is_some() || response.should_close() {
            self.shell.palette.visible = false;
        }
        if let Some(command) = chosen {
            self.run_command(command);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::CommandPalette;

    #[test]
    fn letters_in_order_match() {
        assert!(CommandPalette::matches("Play Live", "plv"));
        assert!(CommandPalette::matches("Select: player", "sel pla"));
        assert!(!CommandPalette::matches("Save", "sv e x"));
    }
}
