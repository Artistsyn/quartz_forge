//! The editor's look: a calm dark theme with one accent colour, roomier spacing and clear type
//! sizes, set once at start-up.

use eframe::egui::{self, Color32, CornerRadius, FontId, Stroke, TextStyle, Theme, Visuals};

/// Colours the shell draws with directly (chips, badges, section headers).
pub(in crate::app) struct ForgeTheme;

impl ForgeTheme {
    pub const ACCENT: Color32 = Color32::from_rgb(86, 148, 255);
    pub const ACCENT_DIM: Color32 = Color32::from_rgb(46, 78, 132);
    pub const GOOD: Color32 = Color32::from_rgb(98, 200, 120);
    pub const WARN: Color32 = Color32::from_rgb(236, 176, 64);
    pub const BAD: Color32 = Color32::from_rgb(236, 96, 88);
    pub const MUTED: Color32 = Color32::from_rgb(140, 146, 158);
    pub const PANEL: Color32 = Color32::from_rgb(30, 32, 38);
    pub const SURFACE: Color32 = Color32::from_rgb(38, 41, 48);
    pub const CANVAS: Color32 = Color32::from_rgb(22, 24, 28);
    pub const BORDER: Color32 = Color32::from_rgb(52, 56, 66);

    pub fn apply(ctx: &egui::Context) {
        ctx.set_theme(Theme::Dark);
        ctx.set_visuals_of(Theme::Dark, Self::visuals());
        ctx.style_mut_of(Theme::Dark, |style| {
            style.spacing.item_spacing = egui::vec2(8.0, 6.0);
            style.spacing.button_padding = egui::vec2(10.0, 4.0);
            style.spacing.interact_size.y = 24.0;
            style.spacing.window_margin = egui::Margin::same(12);
            style.spacing.menu_margin = egui::Margin::same(6);
            style.spacing.indent = 16.0;
            style.text_styles = [
                (TextStyle::Small, FontId::proportional(11.0)),
                (TextStyle::Body, FontId::proportional(13.5)),
                (TextStyle::Button, FontId::proportional(13.5)),
                (TextStyle::Heading, FontId::proportional(18.0)),
                (TextStyle::Monospace, FontId::monospace(12.5)),
            ]
            .into();
        });
    }

    fn visuals() -> Visuals {
        let mut visuals = Visuals::dark();
        let radius = CornerRadius::same(4);
        visuals.panel_fill = Self::PANEL;
        visuals.window_fill = Self::SURFACE;
        visuals.extreme_bg_color = Self::CANVAS;
        visuals.faint_bg_color = Color32::from_rgb(34, 37, 43);
        visuals.window_corner_radius = CornerRadius::same(8);
        visuals.menu_corner_radius = CornerRadius::same(6);
        visuals.window_stroke = Stroke::new(1.0, Self::BORDER);
        visuals.selection.bg_fill = Self::ACCENT_DIM;
        visuals.selection.stroke = Stroke::new(1.0, Self::ACCENT);
        visuals.hyperlink_color = Self::ACCENT;
        for widget in [
            &mut visuals.widgets.noninteractive,
            &mut visuals.widgets.inactive,
            &mut visuals.widgets.hovered,
            &mut visuals.widgets.active,
            &mut visuals.widgets.open,
        ] {
            widget.corner_radius = radius;
        }
        visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, Self::BORDER);
        visuals.widgets.inactive.weak_bg_fill = Color32::from_rgb(48, 52, 61);
        visuals.widgets.inactive.bg_fill = Color32::from_rgb(48, 52, 61);
        visuals.widgets.hovered.weak_bg_fill = Color32::from_rgb(60, 65, 77);
        visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, Self::ACCENT_DIM);
        visuals.widgets.active.weak_bg_fill = Self::ACCENT_DIM;
        visuals
    }

    /// A small rounded label with a coloured outline, for statuses and kinds.
    pub fn chip(ui: &mut egui::Ui, text: &str, color: Color32) -> egui::Response {
        egui::Frame::new()
            .stroke(Stroke::new(1.0, color))
            .corner_radius(CornerRadius::same(9))
            .inner_margin(egui::Margin::symmetric(7, 1))
            .show(ui, |ui| ui.label(egui::RichText::new(text).small().color(color)))
            .response
    }

    /// An upper-case section caption, as panel headers show.
    pub fn caption(ui: &mut egui::Ui, text: &str) {
        ui.label(egui::RichText::new(text.to_uppercase()).small().strong().color(Self::MUTED));
    }
}
