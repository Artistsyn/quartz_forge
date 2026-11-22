//! The "When" half of an event: what it waits for, in plain words, with a key picker that
//! listens for the next key press, and an object picker for who it listens on.

use eframe::egui::{self, RichText};

use super::catalog::Catalog;
use crate::app::fields::Fields;
use crate::app::shell::ForgeTheme;
use crate::core::quartz_domain::{QuartzEventBinding, QuartzEventKind, QuartzKeyModifiers, QuartzMouseButtonFilter, QuartzScrollAxisFilter, QuartzTargetRef};

/// Named keys the engine maps (`Vocabulary::key`, `codegen_text::key_expr`); letters and
/// digits map as characters.
const NAMED_KEYS: [&str; 12] = ["Space", "Enter", "Escape", "Tab", "ArrowLeft", "ArrowRight", "ArrowUp", "ArrowDown", "Backspace", "Delete", "Home", "End"];

pub(in crate::app) struct Trigger;

impl Trigger {
    /// The trigger, its settings, and who it listens on and acts on.
    pub fn fields(ui: &mut egui::Ui, event: &mut QuartzEventBinding, names: &[String]) -> bool {
        Fields::grid(ui, "event_trigger", |fields| {
            fields.text("Name", "", &mut event.name);
            fields.custom("When", "What starts this rule", |ui| Self::kind_picker(ui, &mut event.kind));
            Self::kind_settings(fields, &mut event.kind);
            fields.custom("On", "The object the event listens on (whose key, click or touch)", |ui| Self::target_picker(ui, "listener", &mut event.listener_target, names));
            fields.custom("Acts on", "The object the steps act on unless a step names another", |ui| Self::target_picker(ui, "acts_on", &mut event.action_target, names));
        })
    }

    fn kind_picker(ui: &mut egui::Ui, kind: &mut QuartzEventKind) -> bool {
        let mut picked = None;
        egui::ComboBox::from_id_salt("event_kind").selected_text(Catalog::trigger_label(kind)).width(170.0).show_ui(ui, |ui| {
            for (label, option) in Catalog::triggers() {
                let current = std::mem::discriminant(&option) == std::mem::discriminant(kind);
                if ui.selectable_label(current, label).clicked() && !current {
                    picked = Some(option);
                }
            }
        });
        let Some(mut option) = picked else { return false };
        Self::keep_key(kind, &mut option);
        *kind = option;
        true
    }

    /// Switching between key triggers keeps the key and modifiers.
    fn keep_key(from: &QuartzEventKind, to: &mut QuartzEventKind) {
        let held = match from {
            QuartzEventKind::KeyPress { key, modifiers } | QuartzEventKind::KeyHold { key, modifiers } | QuartzEventKind::KeyRelease { key, modifiers } => (key.clone(), modifiers.clone()),
            _ => return,
        };
        if let QuartzEventKind::KeyPress { key, modifiers } | QuartzEventKind::KeyHold { key, modifiers } | QuartzEventKind::KeyRelease { key, modifiers } = to {
            (*key, *modifiers) = held;
        }
    }

    fn kind_settings(fields: &mut Fields<'_>, kind: &mut QuartzEventKind) {
        match kind {
            QuartzEventKind::KeyPress { key, modifiers } | QuartzEventKind::KeyHold { key, modifiers } | QuartzEventKind::KeyRelease { key, modifiers } => {
                fields.custom("Key", "Click, then press the key", |ui| Self::key_picker(ui, key));
                fields.custom("With", "Keys held together with it", |ui| Self::modifier_toggles(ui, modifiers));
            }
            QuartzEventKind::MousePress { button } | QuartzEventKind::MouseRelease { button } => {
                let buttons = [(QuartzMouseButtonFilter::Left, "Left"), (QuartzMouseButtonFilter::Right, "Right"), (QuartzMouseButtonFilter::Middle, "Middle"), (QuartzMouseButtonFilter::Any, "Any")];
                fields.choice("Button", "", button, &buttons);
            }
            QuartzEventKind::MouseScroll { axis } => {
                let axes = [(QuartzScrollAxisFilter::Any, "Any"), (QuartzScrollAxisFilter::Y, "Up / down"), (QuartzScrollAxisFilter::X, "Sideways")];
                fields.choice("Direction", "", axis, &axes);
            }
            QuartzEventKind::Custom { name } => fields.text("Signal", "Sent by a step or by code", name),
            _ => {}
        }
    }

    /// A button showing the key; clicked, it takes the next key pressed.
    fn key_picker(ui: &mut egui::Ui, key: &mut String) -> bool {
        let listening_id = ui.id().with("key_listening");
        let listening = ui.data(|memory| memory.get_temp::<bool>(listening_id).unwrap_or(false));
        let text = if listening { RichText::new("Press a key…").color(ForgeTheme::WARN) } else { RichText::new(key.as_str()).monospace() };
        let mut changed = false;
        ui.horizontal(|ui| {
            if ui.button(text).clicked() {
                ui.data_mut(|memory| memory.insert_temp(listening_id, !listening));
            }
            if listening && let Some(pressed) = Self::pressed_key(ui) {
                *key = pressed;
                changed = true;
                ui.data_mut(|memory| memory.insert_temp(listening_id, false));
            }
            egui::ComboBox::from_id_salt("common_keys").selected_text("").width(24.0).show_ui(ui, |ui| {
                for name in NAMED_KEYS {
                    changed |= ui.selectable_value(key, name.to_owned(), name).changed();
                }
            });
        });
        changed
    }

    /// The name of a key pressed this frame that the engine can map.
    fn pressed_key(ui: &egui::Ui) -> Option<String> {
        ui.input(|input| {
            input.events.iter().find_map(|event| match event {
                egui::Event::Key { key, pressed: true, .. } => {
                    let name = key.name();
                    let mapped = NAMED_KEYS.contains(&name) || (name.chars().count() == 1 && name.chars().all(|c| c.is_ascii_alphanumeric()));
                    mapped.then(|| name.to_owned())
                }
                _ => None,
            })
        })
    }

    fn modifier_toggles(ui: &mut egui::Ui, modifiers: &mut QuartzKeyModifiers) -> bool {
        ui.horizontal(|ui| {
            let mut changed = ui.toggle_value(&mut modifiers.shift, "Shift").changed();
            changed |= ui.toggle_value(&mut modifiers.control, "Ctrl").changed();
            changed |= ui.toggle_value(&mut modifiers.alt, "Alt").changed();
            changed | ui.toggle_value(&mut modifiers.meta, "Cmd").changed()
        })
        .inner
    }

    /// Pick an object by name, or every object with a tag.
    pub fn target_picker(ui: &mut egui::Ui, id: &str, target: &mut QuartzTargetRef, names: &[String]) -> bool {
        let shown = match target {
            QuartzTargetRef::Name(name) | QuartzTargetRef::Id(name) => name.clone(),
            QuartzTargetRef::Tag(tag) => format!("Tagged {tag}"),
        };
        let mut changed = false;
        ui.horizontal(|ui| {
            egui::ComboBox::from_id_salt(id).selected_text(shown).width(150.0).show_ui(ui, |ui| {
                for name in names {
                    changed |= ui.selectable_value(target, QuartzTargetRef::Name(name.clone()), name).changed();
                }
                ui.separator();
                if ui.selectable_label(matches!(target, QuartzTargetRef::Tag(_)), "Every object with a tag…").clicked() && !matches!(target, QuartzTargetRef::Tag(_)) {
                    *target = QuartzTargetRef::Tag(String::new());
                    changed = true;
                }
            });
            if let QuartzTargetRef::Tag(tag) = target {
                changed |= ui.add(egui::TextEdit::singleline(tag).hint_text("tag").desired_width(80.0)).changed();
            }
        });
        changed
    }
}
