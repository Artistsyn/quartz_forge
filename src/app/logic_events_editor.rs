use eframe::egui;

use crate::app::QuartzForgeApp;
use crate::core::quartz_domain::{
    CompareOp, LogicNode, QuartzAction, QuartzCondition, QuartzEventKind, QuartzLocationRef,
    QuartzMouseButtonFilter, QuartzScrollAxisFilter, QuartzTargetRef,
};

impl QuartzForgeApp {
    #[allow(dead_code)]
    pub(super) fn logic_editor(&mut self, ui: &mut egui::Ui) {
        ui.heading("Update Scripts (on_update)");
        let project_root = self.project_root.clone();
        let logic_rows: Vec<String> = self
            .project_state
            .manifest
            .scenes
            .get(self.project_state.active_scene_index)
            .map(|s| {
                s.logic_trees
                    .iter()
                    .enumerate()
                    .map(|(i, t)| {
                        let file_label = if t.output_file.trim().is_empty() {
                            "<scene file>".to_owned()
                        } else {
                            t.output_file.clone()
                        };
                        format!("[{}] {} ({})", i + 1, t.name, file_label)
                    })
                    .collect()
            })
            .unwrap_or_default();

        for (idx, label) in logic_rows.iter().enumerate() {
            if ui
                .selectable_label(idx == self.selected_logic_tree_index, label)
                .clicked()
            {
                self.selected_logic_tree_index = idx;
            }
        }

        ui.horizontal(|ui| {
            if ui.button("+ Update Script").clicked() {
                self.project_state.add_logic_tree_to_active_scene();
                if let Some(scene) = self
                    .project_state
                    .manifest
                    .scenes
                    .get(self.project_state.active_scene_index)
                {
                    self.selected_logic_tree_index = scene.logic_trees.len().saturating_sub(1);
                }
            }
            if ui.button("- Update Script").clicked() {
                if let Some(scene) = self
                    .project_state
                    .manifest
                    .scenes
                    .get_mut(self.project_state.active_scene_index)
                {
                    if !scene.logic_trees.is_empty() && self.selected_logic_tree_index < scene.logic_trees.len() {
                        scene.logic_trees.remove(self.selected_logic_tree_index);
                        self.selected_logic_tree_index = self.selected_logic_tree_index.saturating_sub(1);
                        self.project_state.dirty = true;
                    }
                }
            }
        });

        ui.separator();
        let mut tree_changed = false;
        {
            let Some(scene) = self
                .project_state
                .manifest
                .scenes
                .get_mut(self.project_state.active_scene_index)
            else {
                return;
            };
            let default_target_name = scene
                .objects
                .get(self.selected_object_index)
                .map(|o| o.id.clone())
                .unwrap_or_else(|| "player".to_owned());
            let Some(tree) = scene.logic_trees.get_mut(self.selected_logic_tree_index) else {
                ui.label("No update script selected.");
                return;
            };

            if ui.text_edit_singleline(&mut tree.name).changed() {
                tree_changed = true;
            }

            ui.separator();
            ui.label("Update Script File Target");
            tree_changed |= Self::source_file_picker(
                ui,
                "Logic File Target",
                project_root.as_deref(),
                &mut self.status_line,
                &mut tree.output_file,
            );

            if ui.button("+ Add Teleport Action").clicked() {
                tree.nodes.push(LogicNode::Action(QuartzAction::Teleport {
                    target: QuartzTargetRef::Name(default_target_name.clone()),
                    location: QuartzLocationRef::At { x: 400.0, y: 300.0 },
                }));
                tree.refresh_references();
                tree_changed = true;
            }

            if ui.button("+ Add KeyHeld Branch").clicked() {
                tree.nodes.push(LogicNode::Branch {
                    condition: QuartzCondition::KeyHeld {
                        key: "space".to_owned(),
                    },
                    then_nodes: vec![LogicNode::Action(QuartzAction::SetMomentum {
                        target: QuartzTargetRef::Name(default_target_name.clone()),
                        mx: 0.0,
                        my: -12.0,
                    })],
                    else_nodes: vec![],
                });
                tree.refresh_references();
                tree_changed = true;
            }

            if ui.button("+ Add Var Compare Branch").clicked() {
                tree.nodes.push(LogicNode::Branch {
                    condition: QuartzCondition::VarCompare {
                        variable: "score".to_owned(),
                        op: CompareOp::Ge,
                        value: 10.0,
                    },
                    then_nodes: vec![LogicNode::Action(QuartzAction::CameraFlash {
                        duration_s: 0.3,
                        intensity: 0.75,
                    })],
                    else_nodes: vec![],
                });
                tree.refresh_references();
                tree_changed = true;
            }

            ui.separator();
            ui.label("Action Chain Outline");
            for (idx, node) in tree.nodes.iter().enumerate() {
                ui.label(format!("{}: {}", idx + 1, node.short_label()));
            }
            ui.separator();
            ui.label("Referenced Objects");
            if tree.referenced_object_ids.is_empty() {
                ui.label("none");
            } else {
                for obj in &tree.referenced_object_ids {
                    ui.label(obj);
                }
            }
        }

        if tree_changed {
            self.touch_and_refresh_preview();
        }
    }

    pub(super) fn events_editor(&mut self, ui: &mut egui::Ui) {
        ui.heading("Events");
        let project_root = self.project_root.clone();
        let event_rows: Vec<String> = self
            .project_state
            .manifest
            .scenes
            .get(self.project_state.active_scene_index)
            .map(|s| {
                s.events
                    .iter()
                    .enumerate()
                    .map(|(i, e)| format!("[{}] {} ({})", i + 1, e.name, e.kind.as_str()))
                    .collect()
            })
            .unwrap_or_default();

        for (idx, label) in event_rows.iter().enumerate() {
            if ui.selectable_label(idx == self.selected_event_index, label).clicked() {
                self.selected_event_index = idx;
            }
        }

        ui.horizontal(|ui| {
            if ui.button("+ Event").clicked() {
                self.project_state.add_event_binding_to_active_scene();
                if let Some(scene) = self
                    .project_state
                    .manifest
                    .scenes
                    .get(self.project_state.active_scene_index)
                {
                    self.selected_event_index = scene.events.len().saturating_sub(1);
                }
                self.touch_and_refresh_preview();
            }
            if ui.button("- Event").clicked() {
                if let Some(scene) = self
                    .project_state
                    .manifest
                    .scenes
                    .get_mut(self.project_state.active_scene_index)
                {
                    if !scene.events.is_empty() && self.selected_event_index < scene.events.len() {
                        scene.events.remove(self.selected_event_index);
                        self.selected_event_index = self.selected_event_index.saturating_sub(1);
                        self.project_state.dirty = true;
                        self.touch_and_refresh_preview();
                        return;
                    }
                }
            }
        });

        ui.separator();

        let mut changed = false;
        {
            let Some(scene) = self
                .project_state
                .manifest
                .scenes
                .get_mut(self.project_state.active_scene_index)
            else {
                return;
            };

            let logic_tree_options: Vec<(String, String)> = scene
                .logic_trees
                .iter()
                .map(|tree| (tree.id.clone(), tree.name.clone()))
                .collect();

            let Some(event) = scene.events.get_mut(self.selected_event_index) else {
                ui.label("No event binding selected.");
                return;
            };

            changed |= ui.text_edit_singleline(&mut event.name).changed();
            changed |= Self::edit_target_ref(ui, "listener target", &mut event.listener_target);
            changed |= Self::edit_target_ref(ui, "action target", &mut event.action_target);

            ui.separator();
            ui.label("Event Code Target");
            changed |= Self::source_file_picker(
                ui,
                "Event File Target",
                project_root.as_deref(),
                &mut self.status_line,
                &mut event.output_file,
            );

            let mut kind_choice = event.kind.as_str();
            egui::ComboBox::from_label("kind")
                .selected_text(kind_choice)
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut kind_choice, "Collision", "Collision");
                    ui.selectable_value(&mut kind_choice, "BoundaryCollision", "BoundaryCollision");
                    ui.selectable_value(&mut kind_choice, "KeyPress", "KeyPress");
                    ui.selectable_value(&mut kind_choice, "KeyRelease", "KeyRelease");
                    ui.selectable_value(&mut kind_choice, "KeyHold", "KeyHold");
                    ui.selectable_value(&mut kind_choice, "Tick", "Tick");
                    ui.selectable_value(&mut kind_choice, "Custom", "Custom");
                    ui.selectable_value(&mut kind_choice, "MousePress", "MousePress");
                    ui.selectable_value(&mut kind_choice, "MouseRelease", "MouseRelease");
                    ui.selectable_value(&mut kind_choice, "MouseEnter", "MouseEnter");
                    ui.selectable_value(&mut kind_choice, "MouseLeave", "MouseLeave");
                    ui.selectable_value(&mut kind_choice, "MouseOver", "MouseOver");
                    ui.selectable_value(&mut kind_choice, "MouseScroll", "MouseScroll");
                    ui.selectable_value(&mut kind_choice, "MouseMove", "MouseMove");
                });

            let replacement = match kind_choice {
                "Collision" => Some(QuartzEventKind::Collision),
                "BoundaryCollision" => Some(QuartzEventKind::BoundaryCollision),
                "KeyPress" => Some(QuartzEventKind::KeyPress {
                    key: "Space".to_owned(),
                    modifiers: Default::default(),
                }),
                "KeyRelease" => Some(QuartzEventKind::KeyRelease {
                    key: "Space".to_owned(),
                    modifiers: Default::default(),
                }),
                "KeyHold" => Some(QuartzEventKind::KeyHold {
                    key: "Space".to_owned(),
                    modifiers: Default::default(),
                }),
                "Tick" => Some(QuartzEventKind::Tick),
                "Custom" => Some(QuartzEventKind::Custom {
                    name: "custom_event".to_owned(),
                }),
                "MousePress" => Some(QuartzEventKind::MousePress {
                    button: QuartzMouseButtonFilter::Any,
                }),
                "MouseRelease" => Some(QuartzEventKind::MouseRelease {
                    button: QuartzMouseButtonFilter::Any,
                }),
                "MouseEnter" => Some(QuartzEventKind::MouseEnter),
                "MouseLeave" => Some(QuartzEventKind::MouseLeave),
                "MouseOver" => Some(QuartzEventKind::MouseOver),
                "MouseScroll" => Some(QuartzEventKind::MouseScroll {
                    axis: QuartzScrollAxisFilter::Any,
                }),
                "MouseMove" => Some(QuartzEventKind::MouseMove),
                _ => None,
            };

            if let Some(next_kind) = replacement {
                if std::mem::discriminant(&event.kind) != std::mem::discriminant(&next_kind) {
                    event.kind = next_kind;
                    changed = true;
                }
            }

            match &mut event.kind {
                QuartzEventKind::KeyPress { key, modifiers }
                | QuartzEventKind::KeyRelease { key, modifiers }
                | QuartzEventKind::KeyHold { key, modifiers } => {
                    ui.label("key");
                    changed |= ui.text_edit_singleline(key).changed();
                    changed |= ui.checkbox(&mut modifiers.shift, "shift").changed();
                    changed |= ui.checkbox(&mut modifiers.control, "control").changed();
                    changed |= ui.checkbox(&mut modifiers.alt, "alt").changed();
                    changed |= ui.checkbox(&mut modifiers.meta, "meta").changed();
                }
                QuartzEventKind::Custom { name } => {
                    ui.label("custom event name");
                    changed |= ui.text_edit_singleline(name).changed();
                }
                QuartzEventKind::MousePress { button }
                | QuartzEventKind::MouseRelease { button } => {
                    egui::ComboBox::from_label("mouse button")
                        .selected_text(button.as_str())
                        .show_ui(ui, |ui| {
                            changed |= ui
                                .selectable_value(button, QuartzMouseButtonFilter::Any, "Any")
                                .changed();
                            changed |= ui
                                .selectable_value(button, QuartzMouseButtonFilter::Left, "Left")
                                .changed();
                            changed |= ui
                                .selectable_value(button, QuartzMouseButtonFilter::Right, "Right")
                                .changed();
                            changed |= ui
                                .selectable_value(button, QuartzMouseButtonFilter::Middle, "Middle")
                                .changed();
                        });
                }
                QuartzEventKind::MouseScroll { axis } => {
                    egui::ComboBox::from_label("scroll axis")
                        .selected_text(axis.as_str())
                        .show_ui(ui, |ui| {
                            changed |= ui
                                .selectable_value(axis, QuartzScrollAxisFilter::Any, "Any")
                                .changed();
                            changed |= ui
                                .selectable_value(axis, QuartzScrollAxisFilter::X, "X")
                                .changed();
                            changed |= ui
                                .selectable_value(axis, QuartzScrollAxisFilter::Y, "Y")
                                .changed();
                        });
                }
                QuartzEventKind::Collision
                | QuartzEventKind::BoundaryCollision
                | QuartzEventKind::Tick
                | QuartzEventKind::MouseEnter
                | QuartzEventKind::MouseLeave
                | QuartzEventKind::MouseOver
                | QuartzEventKind::MouseMove => {}
            }

            let is_custom = matches!(event.kind, QuartzEventKind::Custom { .. });
            if is_custom {
                ui.label("Custom events do not carry an inline Action payload.");
                if event.action.is_some() {
                    event.action = None;
                    changed = true;
                }
            } else {
                if event.action.is_none() {
                    event.action = Some(QuartzAction::Custom {
                        name: "event_action".to_owned(),
                    });
                    changed = true;
                }
                if let Some(action) = event.action.as_mut() {
                    ui.separator();
                    ui.label("Event Action");
                    changed |= Self::edit_action(ui, action);
                }
            }

            ui.separator();
            let selected_tree_text = event
                .linked_logic_tree_id
                .as_ref()
                .and_then(|id| logic_tree_options.iter().find(|(tid, _)| tid == id))
                .map(|(_, name)| name.clone())
                .unwrap_or_else(|| "None (use inline event action)".to_owned());
            egui::ComboBox::from_label("Linked Update Script")
                .selected_text(selected_tree_text)
                .show_ui(ui, |ui| {
                    if ui
                        .selectable_label(event.linked_logic_tree_id.is_none(), "None (use inline event action)")
                        .clicked()
                    {
                        event.linked_logic_tree_id = None;
                        changed = true;
                    }
                    for (tree_id, tree_name) in &logic_tree_options {
                        let selected = event
                            .linked_logic_tree_id
                            .as_ref()
                            .map(|id| id == tree_id)
                            .unwrap_or(false);
                        if ui
                            .selectable_label(selected, format!("{} ({})", tree_name, tree_id))
                            .clicked()
                        {
                            event.linked_logic_tree_id = Some(tree_id.clone());
                            changed = true;
                        }
                    }
                });
            if event.linked_logic_tree_id.is_some() {
                ui.label("Linked update script action chain will be used for generated GameEvent action payload.");
            }

            if changed {
                event.refresh_references();
            }
        }

        if changed {
            self.touch_and_refresh_preview();
        }
    }
}
