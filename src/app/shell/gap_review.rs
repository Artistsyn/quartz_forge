//! Tools > Gap Reports: what agents found missing for a game idea, with their proposed fixes,
//! for the user to approve or reject. Agents file them (`qf_gap_propose`); only this window
//! decides them.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use eframe::egui::{self, RichText};

use super::ForgeTheme;
use crate::app::QuartzForgeApp;
use crate::services::gaps::{GapBook, GapReport, GapStatus};

/// How often the folder is re-read: agents add reports while the editor is open.
const REFRESH: Duration = Duration::from_secs(3);

pub(in crate::app) struct GapReview {
    pub open: bool,
    book: GapBook,
    reports: Vec<GapReport>,
    notes: HashMap<String, String>,
    read_at: Option<Instant>,
    show_decided: bool,
    error: Option<String>,
}

impl GapReview {
    pub fn new(book: GapBook) -> Self {
        Self { open: false, book, reports: Vec::new(), notes: HashMap::new(), read_at: None, show_decided: false, error: None }
    }

    /// Open the window on a fresh read of the folder.
    pub fn show(&mut self) {
        self.open = true;
        self.read_at = None;
    }

    /// Reports waiting for a decision (re-reads the folder when stale).
    pub fn waiting(&mut self) -> usize {
        self.refresh_if_stale();
        self.reports.iter().filter(|report| report.status == GapStatus::Proposed).count()
    }

    fn refresh_if_stale(&mut self) {
        if self.read_at.is_some_and(|at| at.elapsed() < REFRESH) {
            return;
        }
        self.read_at = Some(Instant::now());
        match self.book.list() {
            Ok(reports) => {
                self.reports = reports;
                self.error = None;
            }
            Err(error) => self.error = Some(format!("{error:#}")),
        }
    }

    fn decide(&mut self, id: &str, status: GapStatus) {
        let note = self.notes.remove(id).unwrap_or_default();
        if let Err(error) = self.book.decide(id, status, &note) {
            self.error = Some(format!("{error:#}"));
        }
        self.read_at = None;
    }

    fn body(&mut self, ui: &mut egui::Ui) {
        self.refresh_if_stale();
        ui.horizontal(|ui| {
            ui.label(RichText::new("Agents file a gap when the engine or the editor cannot yet do what a game needs.").color(ForgeTheme::MUTED));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.checkbox(&mut self.show_decided, "Show decided");
            });
        });
        if let Some(error) = &self.error {
            ui.colored_label(ForgeTheme::BAD, error);
        }
        ui.separator();
        let shown: Vec<GapReport> =
            self.reports.iter().filter(|report| self.show_decided || report.status == GapStatus::Proposed).cloned().collect();
        if shown.is_empty() {
            ui.add_space(24.0);
            ui.vertical_centered(|ui| ui.label(RichText::new("No gap reports waiting for a decision.").color(ForgeTheme::MUTED)));
            return;
        }
        egui::ScrollArea::vertical().id_salt("gap_reports_scroll").show(ui, |ui| {
            for report in &shown {
                self.card(ui, report);
                ui.add_space(8.0);
            }
        });
    }

    fn card(&mut self, ui: &mut egui::Ui, report: &GapReport) {
        egui::Frame::new()
            .fill(ForgeTheme::PANEL)
            .stroke(egui::Stroke::new(1.0, ForgeTheme::BORDER))
            .corner_radius(6)
            .inner_margin(egui::Margin::same(12))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.label(RichText::new(&report.title).strong().size(15.0));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let (text, color) = Self::status_chip(report.status);
                        ForgeTheme::chip(ui, text, color);
                        ForgeTheme::chip(ui, &format!("{:?}", report.area), ForgeTheme::ACCENT);
                    });
                });
                let context = [report.game.as_str(), report.effort.as_str()].iter().filter(|part| !part.is_empty()).copied().collect::<Vec<_>>().join("  ·  ");
                ui.label(RichText::new(context).small().color(ForgeTheme::MUTED));
                ui.add_space(4.0);
                ForgeTheme::caption(ui, "Needed");
                ui.label(&report.need);
                ForgeTheme::caption(ui, "Proposed implementation");
                ui.label(&report.proposal);
                self.decision_row(ui, report);
            });
    }

    fn decision_row(&mut self, ui: &mut egui::Ui, report: &GapReport) {
        if report.status != GapStatus::Proposed {
            if !report.decision.is_empty() {
                ForgeTheme::caption(ui, "Decision note");
                ui.label(&report.decision);
            }
            return;
        }
        ui.add_space(6.0);
        let note = self.notes.entry(report.id.clone()).or_default();
        ui.add(egui::TextEdit::singleline(note).hint_text("Note for the agent (optional)").desired_width(f32::INFINITY));
        ui.horizontal(|ui| {
            if ui.add(egui::Button::new(RichText::new("Approve").color(egui::Color32::WHITE)).fill(ForgeTheme::ACCENT_DIM)).clicked() {
                self.decide(&report.id, GapStatus::Approved);
            }
            if ui.button("Reject").clicked() {
                self.decide(&report.id, GapStatus::Rejected);
            }
        });
    }

    fn status_chip(status: GapStatus) -> (&'static str, egui::Color32) {
        match status {
            GapStatus::Proposed => ("Waiting", ForgeTheme::WARN),
            GapStatus::Approved => ("Approved", ForgeTheme::GOOD),
            GapStatus::Rejected => ("Rejected", ForgeTheme::MUTED),
            GapStatus::Done => ("Done", ForgeTheme::ACCENT),
        }
    }
}

impl QuartzForgeApp {
    pub(in crate::app) fn gap_review_window(&mut self, ctx: &egui::Context) {
        let mut open = self.shell.gaps.open;
        egui::Window::new("Gap Reports")
            .open(&mut open)
            .default_size(egui::vec2(680.0, 560.0))
            .pivot(egui::Align2::CENTER_CENTER)
            .default_pos(ctx.content_rect().center())
            .collapsible(false)
            .show(ctx, |ui| self.shell.gaps.body(ui));
        self.shell.gaps.open = open;
    }
}
