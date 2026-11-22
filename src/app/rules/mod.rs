//! The Events & Logic tab: every rule of the scene in a list (events, and rules that run every
//! frame), and the selected rule as "When … Do …" with its steps as blocks.

mod blocks;
mod catalog;
mod trigger;

pub(in crate::app) use catalog::Catalog;

use eframe::egui::{self, RichText};

use blocks::{BlockContext, Blocks};
use trigger::Trigger;

use super::fields::Fields;
use super::shell::ForgeTheme;
use super::QuartzForgeApp;
use crate::core::project::SceneDocument;
use crate::core::quartz_domain::{LogicTree, QuartzAction, QuartzCondition, QuartzEventBinding, QuartzEventKind, QuartzTargetRef};
use crate::core::samples::Samples;

/// PathForge commands offered as ready-made steps: (label, `RunPlugin` data).
const PATH_FORGE_STEPS: [(&str, &str); 11] = [
    ("Walk on to the next place", "next"),
    ("Take the left branch", "choose:left"),
    ("Take the right branch", "choose:right"),
    ("Stop walking", "stop"),
    ("Walk", "walk"),
    ("Set the hour to noon", "time:12"),
    ("Set the hour to night", "time:23"),
    ("Let days pass (10 min a day)", "day:600"),
    ("Replay the intro", "intro"),
    ("Swing to side view", "swing:side"),
    ("Swing back to home view", "swing:home"),
];

/// Which rule the editor shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::app) enum RuleFocus {
    Event(usize),
    EveryFrame(usize),
}

impl QuartzForgeApp {
    pub(super) fn rules_editor(&mut self, ui: &mut egui::Ui) {
        egui::Panel::left("rule_list").resizable(true).default_size(280.0).min_size(200.0).show(ui, |ui| {
            egui::ScrollArea::vertical().id_salt("rule_list_scroll").show(ui, |ui| self.rule_list(ui));
        });
        egui::CentralPanel::default().show(ui, |ui| {
            egui::ScrollArea::vertical().id_salt("rule_detail_scroll").auto_shrink([false, false]).show(ui, |ui| self.rule_detail(ui));
        });
    }

    fn rule_list(&mut self, ui: &mut egui::Ui) {
        let Some(scene) = self.project_state.manifest.scenes.get(self.project_state.active_scene_index) else { return };
        let events: Vec<(String, String)> = scene.events.iter().map(|event| (event.name.clone(), Self::trigger_summary(&event.kind))).collect();
        let every_frame: Vec<(String, usize)> = scene.logic_trees.iter().map(|tree| (tree.name.clone(), tree.nodes.len())).collect();
        if Self::list_header(ui, "Events", "New event: when something happens, do steps") {
            self.add_event_rule();
        }
        if events.is_empty() {
            ui.label(RichText::new("Keys, clicks and touches start these.").small().color(ForgeTheme::MUTED));
        }
        for (index, (name, trigger)) in events.iter().enumerate() {
            self.rule_row(ui, RuleFocus::Event(index), name, trigger);
        }
        ui.add_space(8.0);
        if Self::list_header(ui, "Every frame", "New rule that runs its steps every frame") {
            self.add_every_frame_rule();
        }
        if every_frame.is_empty() {
            ui.label(RichText::new("Steps here run every frame, often behind an If.").small().color(ForgeTheme::MUTED));
        }
        for (index, (name, steps)) in every_frame.iter().enumerate() {
            self.rule_row(ui, RuleFocus::EveryFrame(index), name, &format!("{steps} step{}", if *steps == 1 { "" } else { "s" }));
        }
    }

    fn list_header(ui: &mut egui::Ui, title: &str, hint: &str) -> bool {
        ui.horizontal(|ui| {
            ForgeTheme::caption(ui, title);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| ui.small_button("+").on_hover_text(hint).clicked()).inner
        })
        .inner
    }

    fn rule_row(&mut self, ui: &mut egui::Ui, focus: RuleFocus, name: &str, note: &str) {
        let selected = self.shell.tabs.rule == focus;
        let response = ui.add(
            egui::Button::selectable(selected, name).right_text(RichText::new(note).small().color(ForgeTheme::MUTED)).min_size(egui::vec2(ui.available_width(), 0.0)),
        );
        if response.clicked() {
            self.shell.tabs.rule = focus;
        }
        response.context_menu(|ui| {
            if ui.button("Duplicate").clicked() {
                self.duplicate_rule(focus);
            }
            if ui.button("Delete").clicked() {
                self.delete_rule(focus);
            }
        });
    }

    fn trigger_summary(kind: &QuartzEventKind) -> String {
        match kind {
            QuartzEventKind::KeyPress { key, .. } | QuartzEventKind::KeyHold { key, .. } | QuartzEventKind::KeyRelease { key, .. } => {
                format!("{} {key}", Catalog::trigger_label(kind))
            }
            other => Catalog::trigger_label(other).to_owned(),
        }
    }

    fn rule_detail(&mut self, ui: &mut egui::Ui) {
        let suggestions = self.current_editor_suggestions();
        let scene_index = self.project_state.active_scene_index;
        let Some(scene) = self.project_state.manifest.scenes.get_mut(scene_index) else { return };
        let names: Vec<String> = scene.objects.iter().filter(|blueprint| !blueprint.spawn_only).map(|blueprint| blueprint.id.clone()).collect();
        let template = scene.objects.iter().find(|blueprint| blueprint.spawn_only).map_or_else(|| "bullet".to_owned(), |blueprint| blueprint.id.clone());
        let default_target = Self::focus_target(scene, self.shell.tabs.rule, &names);
        let samples = Samples {
            other: QuartzTargetRef::Name(names.iter().find(|name| QuartzTargetRef::Name((*name).clone()) != default_target).cloned().unwrap_or_default()),
            target: default_target,
            template_id: template,
            animation_asset: String::new(),
        };
        let mut menu = Catalog::menu(&samples);
        menu.extend(Self::plugin_steps(&suggestions));
        let context = BlockContext { suggestions: &suggestions, menu: &menu, new_condition: QuartzCondition::KeyHeld { key: "Space".to_owned() } };
        let root = self.project_root.clone();
        let changed = match self.shell.tabs.rule {
            RuleFocus::Event(index) => Self::event_detail(ui, scene, index, &names, &context),
            RuleFocus::EveryFrame(index) => Self::every_frame_detail(ui, scene, index, &context),
        };
        let file = Self::rule_file(scene, self.shell.tabs.rule);
        let file_changed = file.is_some_and(|file| Fields::section(ui, "Code File", false, |ui| Self::source_file_picker(ui, "File", root.as_deref(), &mut self.status_line, file)));
        if changed || file_changed {
            self.touch_and_refresh_preview();
        }
    }

    /// Ready-made plugin steps for what the scene uses: PathForge walking and background changes.
    fn plugin_steps(suggestions: &crate::app::EditorSuggestions) -> Vec<(&'static str, Vec<(String, QuartzAction)>)> {
        let run = |name: &str, command: String| QuartzAction::RunPlugin { name: name.to_owned(), data: command };
        let mut groups = Vec::new();
        if suggestions.path_forge {
            let steps = PATH_FORGE_STEPS.iter().map(|(label, command)| ((*label).to_owned(), run("path_forge", (*command).to_owned()))).collect();
            groups.push(("PathForge", steps));
        }
        if let Some(first) = suggestions.background_keys.first() {
            let second = suggestions.background_keys.get(1).unwrap_or(first);
            let steps = vec![
                ("Crossfade background".to_owned(), run("background", format!("transition:{first},{second},1"))),
                ("Switch background".to_owned(), run("background", format!("set:{first}"))),
            ];
            groups.push(("Background", steps));
        }
        groups
    }

    /// The object a new step acts on: the rule's own target, else the first object.
    fn focus_target(scene: &SceneDocument, focus: RuleFocus, names: &[String]) -> QuartzTargetRef {
        let first = || QuartzTargetRef::Name(names.first().cloned().unwrap_or_else(|| "player".to_owned()));
        match focus {
            RuleFocus::Event(index) => scene.events.get(index).map_or_else(first, |event| event.action_target.clone()),
            RuleFocus::EveryFrame(_) => first(),
        }
    }

    fn rule_file(scene: &mut SceneDocument, focus: RuleFocus) -> Option<&mut String> {
        match focus {
            RuleFocus::Event(index) => scene.events.get_mut(index).map(|event| &mut event.output_file),
            RuleFocus::EveryFrame(index) => scene.logic_trees.get_mut(index).map(|tree| &mut tree.output_file),
        }
    }

    fn event_detail(ui: &mut egui::Ui, scene: &mut SceneDocument, index: usize, names: &[String], context: &BlockContext<'_>) -> bool {
        let trees: Vec<(String, String)> = scene.logic_trees.iter().map(|tree| (tree.id.clone(), tree.name.clone())).collect();
        let Some(event) = scene.events.get_mut(index) else {
            Self::empty_detail(ui);
            return false;
        };
        let wide = ui.available_width() > 860.0;
        let mut changed = false;
        if wide {
            ui.columns(2, |columns| {
                changed |= Self::event_when(&mut columns[0], event, names, &trees);
                changed |= Self::event_do(&mut columns[1], event, context);
            });
        } else {
            changed |= Self::event_when(ui, event, names, &trees);
            changed |= Self::event_do(ui, event, context);
        }
        if changed {
            event.refresh_references();
        }
        changed
    }

    fn event_when(ui: &mut egui::Ui, event: &mut QuartzEventBinding, names: &[String], trees: &[(String, String)]) -> bool {
        ForgeTheme::caption(ui, "When");
        let mut changed = Trigger::fields(ui, event, names);
        changed |= Fields::grid(ui, "event_run", |fields| {
            let mut options: Vec<(Option<String>, &str)> = vec![(None, "Its own steps")];
            options.extend(trees.iter().map(|(id, name)| (Some(id.clone()), name.as_str())));
            let mut linked = event.linked_logic_tree_id.clone();
            fields.custom("Runs", "Its own steps, or the steps of an every-frame rule", |ui| Self::linked_picker(ui, &mut linked, &options));
            event.linked_logic_tree_id = linked;
        });
        changed
    }

    fn event_do(ui: &mut egui::Ui, event: &mut QuartzEventBinding, context: &BlockContext<'_>) -> bool {
        let mut changed = false;
        ForgeTheme::caption(ui, "Do");
        if event.linked_logic_tree_id.is_some() {
            ui.label(RichText::new("Runs the steps of the chosen every-frame rule.").color(ForgeTheme::MUTED));
        } else if matches!(event.kind, QuartzEventKind::Custom { .. }) {
            ui.label(RichText::new("A custom signal carries no steps of its own; link a rule to run.").color(ForgeTheme::MUTED));
        } else {
            let mut steps = Blocks::steps_of(event.action.as_ref());
            if Blocks::show(ui, &mut steps, context) {
                event.action = Blocks::to_action(&steps);
                changed = true;
            }
        }
        changed
    }

    fn linked_picker(ui: &mut egui::Ui, linked: &mut Option<String>, options: &[(Option<String>, &str)]) -> bool {
        let current = options.iter().find(|(id, _)| id == linked).map_or("Its own steps", |(_, name)| *name);
        let mut changed = false;
        egui::ComboBox::from_id_salt("event_linked_rule").selected_text(current).width(170.0).show_ui(ui, |ui| {
            for (id, name) in options {
                changed |= ui.selectable_value(linked, id.clone(), *name).changed();
            }
        });
        changed
    }

    fn every_frame_detail(ui: &mut egui::Ui, scene: &mut SceneDocument, index: usize, context: &BlockContext<'_>) -> bool {
        let Some(tree) = scene.logic_trees.get_mut(index) else {
            Self::empty_detail(ui);
            return false;
        };
        let mut changed = Fields::grid(ui, "every_frame_rule", |fields| fields.text("Name", "", &mut tree.name));
        ui.add_space(6.0);
        ForgeTheme::caption(ui, "Every frame, do");
        changed |= Blocks::show(ui, &mut tree.nodes, context);
        if changed {
            tree.refresh_references();
        }
        changed
    }

    fn empty_detail(ui: &mut egui::Ui) {
        ui.add_space(24.0);
        ui.vertical_centered(|ui| {
            ui.label(RichText::new("Pick a rule on the left, or add one with +.").color(ForgeTheme::MUTED));
            ui.label(RichText::new("An event waits for a key, a click or a touch; an every-frame rule runs all the time.").small().color(ForgeTheme::MUTED));
        });
    }

    fn add_event_rule(&mut self) {
        self.project_state.add_event_binding_to_active_scene();
        let scene_index = self.project_state.active_scene_index;
        if let Some(event) = self.project_state.manifest.scenes.get_mut(scene_index).and_then(|scene| scene.events.last_mut()) {
            event.action = None;
        }
        let count = self.project_state.manifest.scenes.get(scene_index).map_or(0, |scene| scene.events.len());
        self.shell.tabs.rule = RuleFocus::Event(count.saturating_sub(1));
        self.touch_and_refresh_preview();
    }

    fn add_every_frame_rule(&mut self) {
        self.project_state.add_logic_tree_to_active_scene();
        let count = self.project_state.manifest.scenes.get(self.project_state.active_scene_index).map_or(0, |scene| scene.logic_trees.len());
        self.shell.tabs.rule = RuleFocus::EveryFrame(count.saturating_sub(1));
        self.touch_and_refresh_preview();
    }

    fn duplicate_rule(&mut self, focus: RuleFocus) {
        let scene_index = self.project_state.active_scene_index;
        let manifest = &mut self.project_state.manifest;
        let Some(scene) = manifest.scenes.get(scene_index) else { return };
        match focus {
            RuleFocus::Event(index) => {
                let Some(mut copy) = scene.events.get(index).cloned() else { return };
                (copy.id, _) = manifest.next_event_identity();
                copy.name = format!("{} copy", copy.name);
                manifest.scenes[scene_index].events.insert(index + 1, copy);
                self.shell.tabs.rule = RuleFocus::Event(index + 1);
            }
            RuleFocus::EveryFrame(index) => {
                let Some(mut copy): Option<LogicTree> = scene.logic_trees.get(index).cloned() else { return };
                (copy.id, _) = manifest.next_logic_tree_identity();
                copy.name = format!("{} copy", copy.name);
                manifest.scenes[scene_index].logic_trees.insert(index + 1, copy);
                self.shell.tabs.rule = RuleFocus::EveryFrame(index + 1);
            }
        }
        self.touch_and_refresh_preview();
    }

    fn delete_rule(&mut self, focus: RuleFocus) {
        let scene_index = self.project_state.active_scene_index;
        let Some(scene) = self.project_state.manifest.scenes.get_mut(scene_index) else { return };
        let removed = match focus {
            RuleFocus::Event(index) if index < scene.events.len() => scene.events.remove(index).name,
            RuleFocus::EveryFrame(index) if index < scene.logic_trees.len() => scene.logic_trees.remove(index).name,
            _ => return,
        };
        self.status_line = format!("Deleted rule {removed} (Undo brings it back)");
        self.touch_and_refresh_preview();
    }
}
