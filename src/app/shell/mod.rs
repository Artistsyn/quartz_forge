//! The editor's frame around the editing panels: menu bar, toolbar, docked panels around the
//! scene view, status bar, welcome screen, command palette, undo history and gap reports. The
//! editing panels themselves (object details, events, code blocks, lighting) live in the parent
//! module and are placed here; nothing opens over the scene view unless the user asks.

mod actions;
mod dock;
mod gap_review;
mod history;
mod menu;
mod palette;
mod recent;
mod templates;
mod theme;
mod welcome;

use std::time::Duration;

use eframe::egui;

pub(in crate::app) use actions::NewObjectKind;
pub(in crate::app) use recent::RecentProjects;
pub(in crate::app) use templates::ProjectTemplate;
pub(in crate::app) use theme::ForgeTheme;

use crate::app::QuartzForgeApp;
use crate::core::quartz_domain::CustomCodeKind;
use crate::services::gaps::GapBook;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::app) enum CenterTab {
    Scene,
    Camera,
    Code,
}

impl CenterTab {
    pub const ALL: [Self; 3] = [Self::Scene, Self::Camera, Self::Code];

    pub fn label(self) -> &'static str {
        match self {
            Self::Scene => "Scene View",
            Self::Camera => "Camera View",
            Self::Code => "Code",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(in crate::app) enum DockTab {
    Events,
    CodeBlocks,
    Background,
    Lighting,
    Effects,
    Files,
    Output,
}

impl DockTab {
    pub const ALL: [Self; 7] = [Self::Events, Self::CodeBlocks, Self::Background, Self::Lighting, Self::Effects, Self::Files, Self::Output];

    pub fn label(self) -> &'static str {
        match self {
            Self::Events => "Events & Logic",
            Self::CodeBlocks => "Code Blocks",
            Self::Background => "Background",
            Self::Lighting => "Lighting",
            Self::Effects => "Effects",
            Self::Files => "Files",
            Self::Output => "Output",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::app) enum InspectorTab {
    Object,
    Scene,
}

/// Which code blocks the Code Blocks tab lists.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::app) enum CodeView {
    Kind(CustomCodeKind, &'static str),
    /// File overrides and any block kind without a tab of its own.
    Other,
}

impl CodeView {
    pub const ALL: [Self; 7] = [
        Self::Kind(CustomCodeKind::Constants, "Constants"),
        Self::Kind(CustomCodeKind::GameStateVars, "Game State"),
        Self::Kind(CustomCodeKind::TypedVars, "Typed Variables"),
        Self::Kind(CustomCodeKind::CustomEvents, "Custom Events"),
        Self::Kind(CustomCodeKind::UpdateLoops, "Update Loops"),
        Self::Kind(CustomCodeKind::TopLevel, "Top Level"),
        Self::Other,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Kind(_, label) => label,
            Self::Other => "File Overrides",
        }
    }
}

/// Every status message of the session, with the time it appeared.
#[derive(Debug, Default)]
pub(in crate::app) struct OutputLog {
    pub lines: Vec<(String, String)>,
}

impl OutputLog {
    fn note(&mut self, status: &str) {
        if status.is_empty() || self.lines.last().is_some_and(|(_, last)| last == status) {
            return;
        }
        self.lines.push((chrono::Local::now().format("%H:%M:%S").to_string(), status.to_owned()));
    }
}

/// Which tab each panel shows, and whether the bottom panel is open.
#[derive(Debug, Clone, Copy)]
pub(in crate::app) struct Tabs {
    pub center: CenterTab,
    pub dock: DockTab,
    pub inspector: InspectorTab,
    pub code_view: CodeView,
    pub dock_open: bool,
    pub rule: crate::app::rules::RuleFocus,
}

pub(in crate::app) struct Shell {
    pub tabs: Tabs,
    pub show_shortcuts: bool,
    pub hierarchy_filter: String,
    pub recent: RecentProjects,
    pub welcome: welcome::WelcomeForm,
    pub gaps: gap_review::GapReview,
    pub palette: palette::CommandPalette,
    pub history: history::History,
    pub output: OutputLog,
    /// Frame every object in the scene view on its next frame.
    pub fit_requested: bool,
    /// The scene the view last checked for being framed on first sight.
    pub framed_scene: Option<String>,
    theme_applied: bool,
}

impl Shell {
    pub fn new(recent: RecentProjects, gaps: GapBook) -> Self {
        Self {
            tabs: Tabs {
                center: CenterTab::Scene,
                dock: DockTab::Events,
                inspector: InspectorTab::Object,
                code_view: CodeView::ALL[0],
                dock_open: true,
                rule: crate::app::rules::RuleFocus::Event(0),
            },
            show_shortcuts: false,
            hierarchy_filter: String::new(),
            recent,
            welcome: welcome::WelcomeForm::default(),
            gaps: gap_review::GapReview::new(gaps),
            palette: palette::CommandPalette::default(),
            history: history::History::default(),
            output: OutputLog::default(),
            fit_requested: false,
            framed_scene: None,
            theme_applied: false,
        }
    }
}

impl QuartzForgeApp {
    /// The whole window, each frame.
    pub(in crate::app) fn shell_ui(&mut self, root: &mut egui::Ui) {
        let ctx = root.ctx().clone();
        if !self.shell.theme_applied {
            ForgeTheme::apply(&ctx);
            self.shell.theme_applied = true;
        }
        self.apply_shortcuts(&ctx);
        egui::Panel::top("menu_bar").show(root, |ui| self.menu_bar(ui));
        if self.project_root.is_none() && self.show_startup_prompt {
            egui::CentralPanel::default().show(root, |ui| self.welcome_screen(ui));
        } else {
            self.workspace(root);
        }
        self.overlays(&ctx);
        self.shell.output.note(&self.status_line);
        self.observe_history(&ctx);
        if self.shell.gaps.open {
            ctx.request_repaint_after(Duration::from_secs(3));
        }
    }

    fn workspace(&mut self, root: &mut egui::Ui) {
        egui::Panel::top("toolbar").show(root, |ui| {
            ui.add_space(3.0);
            self.toolbar(ui);
            ui.add_space(3.0);
        });
        egui::Panel::bottom("status_bar").show(root, |ui| self.status_bar(ui));
        if self.shell.tabs.dock_open {
            egui::Panel::bottom("dock").resizable(true).default_size(330.0).min_size(120.0).show(root, |ui| self.dock_panel(ui));
        }
        egui::Panel::left("outliner").resizable(true).default_size(240.0).min_size(180.0).show(root, |ui| self.outliner(ui));
        egui::Panel::right("inspector").resizable(true).default_size(360.0).min_size(260.0).show(root, |ui| self.inspector(ui));
        egui::CentralPanel::default().show(root, |ui| {
            if !self.shell.tabs.dock_open && ui.small_button("⬆ Show bottom panel").clicked() {
                self.shell.tabs.dock_open = true;
            }
            self.center(ui);
        });
    }

    fn overlays(&mut self, ctx: &egui::Context) {
        self.floating_windows(ctx);
        self.gap_review_window(ctx);
        self.shortcuts_window(ctx);
        self.new_project_dialog(ctx);
        self.command_palette(ctx);
    }
}
