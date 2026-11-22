//! The steps of a rule as a list of blocks: actions, and If/Else blocks holding lists of their
//! own. Each block shows its plain name and a summary, opens to its settings, and can be moved,
//! duplicated or deleted. An event stores its steps as one action (`Multi`, `Conditional`); the
//! editor shows them as the same blocks an every-frame rule has.

use eframe::egui::{self, RichText};

use super::catalog::Catalog;
use crate::app::shell::ForgeTheme;
use crate::app::{EditorSuggestions, QuartzForgeApp};
use crate::core::quartz_domain::{LogicNode, QuartzAction, QuartzCondition};

/// Everything a block list needs besides its blocks.
pub(in crate::app) struct BlockContext<'a> {
    pub suggestions: &'a EditorSuggestions,
    pub menu: &'a [(&'static str, Vec<(String, QuartzAction)>)],
    pub new_condition: QuartzCondition,
}

/// What a block's own buttons asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BlockRequest {
    Stay,
    Up,
    Down,
    Duplicate,
    Delete,
}

pub(in crate::app) struct Blocks;

impl Blocks {
    /// An event's action as blocks.
    pub fn steps_of(action: Option<&QuartzAction>) -> Vec<LogicNode> {
        action.map(Self::nodes_of).unwrap_or_default()
    }

    fn nodes_of(action: &QuartzAction) -> Vec<LogicNode> {
        match action {
            QuartzAction::Multi { actions } => actions.iter().flat_map(Self::nodes_of).collect(),
            QuartzAction::Conditional { condition, if_true, if_false } => vec![LogicNode::Branch {
                condition: condition.clone(),
                then_nodes: Self::nodes_of(if_true),
                else_nodes: if_false.as_deref().map(Self::nodes_of).unwrap_or_default(),
            }],
            single => vec![LogicNode::Action(single.clone())],
        }
    }

    /// Blocks as one event action: nothing, the single step, or `Multi` of the steps.
    pub fn to_action(nodes: &[LogicNode]) -> Option<QuartzAction> {
        match nodes {
            [] => None,
            [single] => Some(Self::action_of(single)),
            many => Some(QuartzAction::Multi { actions: many.iter().map(Self::action_of).collect() }),
        }
    }

    fn action_of(node: &LogicNode) -> QuartzAction {
        match node {
            LogicNode::Action(action) => action.clone(),
            LogicNode::Branch { condition, then_nodes, else_nodes } => QuartzAction::Conditional {
                condition: condition.clone(),
                if_true: Box::new(Self::to_action(then_nodes).unwrap_or(QuartzAction::Multi { actions: Vec::new() })),
                if_false: Self::to_action(else_nodes).map(Box::new),
            },
        }
    }

    /// The list editor: every block, then the Add menu.
    pub fn show(ui: &mut egui::Ui, nodes: &mut Vec<LogicNode>, context: &BlockContext<'_>) -> bool {
        let opened = ui.id().with("opened_block");
        let open_index: Option<usize> = ui.data_mut(|memory| memory.remove_temp(opened));
        let mut changed = false;
        let mut request = (0, BlockRequest::Stay);
        for (index, node) in nodes.iter_mut().enumerate() {
            let (edited, asked) = ui.push_id(index, |ui| Self::block(ui, node, context, open_index == Some(index))).inner;
            changed |= edited;
            if asked != BlockRequest::Stay {
                request = (index, asked);
            }
        }
        changed |= Self::apply(nodes, request);
        if let Some(added) = Self::add_menu(ui, context) {
            nodes.push(added);
            ui.data_mut(|memory| memory.insert_temp(opened, nodes.len() - 1));
            changed = true;
        }
        changed
    }

    fn apply(nodes: &mut Vec<LogicNode>, (index, request): (usize, BlockRequest)) -> bool {
        match request {
            BlockRequest::Stay => return false,
            BlockRequest::Up if index > 0 => nodes.swap(index, index - 1),
            BlockRequest::Down if index + 1 < nodes.len() => nodes.swap(index, index + 1),
            BlockRequest::Duplicate => nodes.insert(index + 1, nodes[index].clone()),
            BlockRequest::Delete => {
                nodes.remove(index);
            }
            BlockRequest::Up | BlockRequest::Down => return false,
        }
        true
    }

    fn block(ui: &mut egui::Ui, node: &mut LogicNode, context: &BlockContext<'_>, open_now: bool) -> (bool, BlockRequest) {
        let accent = if matches!(node, LogicNode::Branch { .. }) { ForgeTheme::WARN } else { ForgeTheme::ACCENT };
        let shown = egui::Frame::new()
            .fill(ForgeTheme::SURFACE)
            .stroke(egui::Stroke::new(1.0, ForgeTheme::BORDER))
            .corner_radius(5)
            .inner_margin(egui::Margin { left: 10, right: 8, top: 5, bottom: 5 })
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                let id = ui.id().with("block_open");
                let mut state = egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), id, false);
                if open_now {
                    state.set_open(true);
                }
                let mut request = BlockRequest::Stay;
                ui.horizontal(|ui| {
                    state.show_toggle_button(ui, egui::collapsing_header::paint_default_icon);
                    let (title, summary) = Self::heading(node);
                    if ui.add(egui::Label::new(RichText::new(title).strong()).sense(egui::Sense::click())).clicked() {
                        state.toggle(ui);
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        request = Self::block_buttons(ui);
                        ui.add(egui::Label::new(RichText::new(summary).small().color(ForgeTheme::MUTED)).truncate());
                    });
                });
                let body = state.show_body_unindented(ui, |ui| Self::block_body(ui, node, context));
                (body.is_some_and(|inner| inner.inner), request)
            });
        let frame = shown.response.rect;
        let accent_bar = egui::Rect::from_min_max(frame.left_top() + egui::vec2(1.0, 4.0), egui::pos2(frame.left() + 4.0, frame.bottom() - 4.0));
        ui.painter().rect_filled(accent_bar, 1.5, accent);
        shown.inner
    }

    fn heading(node: &LogicNode) -> (String, String) {
        match node {
            LogicNode::Action(action) => (Catalog::action_label(action), action.short_label()),
            LogicNode::Branch { condition, then_nodes, else_nodes } => {
                let steps = format!("{} then, {} else", then_nodes.len(), else_nodes.len());
                (format!("If {}", Catalog::condition_label(condition).to_lowercase()), steps)
            }
        }
    }

    fn block_buttons(ui: &mut egui::Ui) -> BlockRequest {
        let mut request = BlockRequest::Stay;
        for (glyph, hint, asked) in [("✖", "Delete", BlockRequest::Delete), ("📋", "Duplicate", BlockRequest::Duplicate), ("⬇", "Move down", BlockRequest::Down), ("⬆", "Move up", BlockRequest::Up)] {
            if ui.small_button(glyph).on_hover_text(hint).clicked() {
                request = asked;
            }
        }
        request
    }

    fn block_body(ui: &mut egui::Ui, node: &mut LogicNode, context: &BlockContext<'_>) -> bool {
        ui.add_space(4.0);
        match node {
            LogicNode::Action(action) => QuartzForgeApp::edit_action(ui, action, context.suggestions),
            LogicNode::Branch { condition, then_nodes, else_nodes } => {
                let mut changed = QuartzForgeApp::edit_condition(ui, condition, context.suggestions);
                ForgeTheme::caption(ui, "Then");
                changed |= ui.indent("then", |ui| Self::show(ui, then_nodes, context)).inner;
                ForgeTheme::caption(ui, "Else");
                changed |= ui.indent("else", |ui| Self::show(ui, else_nodes, context)).inner;
                changed
            }
        }
    }

    fn add_menu(ui: &mut egui::Ui, context: &BlockContext<'_>) -> Option<LogicNode> {
        let mut added = None;
        ui.menu_button(RichText::new("+ Add step").color(ForgeTheme::ACCENT), |ui| {
            if ui.button("If / Else").on_hover_text("Run steps only when a condition holds").clicked() {
                added = Some(LogicNode::Branch { condition: context.new_condition.clone(), then_nodes: Vec::new(), else_nodes: Vec::new() });
            }
            ui.separator();
            for (category, entries) in context.menu {
                ui.menu_button(*category, |ui| {
                    if let Some(action) = Self::category_menu(ui, entries) {
                        added = Some(LogicNode::Action(action));
                    }
                });
            }
        });
        added
    }

    fn category_menu(ui: &mut egui::Ui, entries: &[(String, QuartzAction)]) -> Option<QuartzAction> {
        egui::ScrollArea::vertical()
            .max_height(420.0)
            .show(ui, |ui| entries.iter().find(|(label, _)| ui.button(label).clicked()).map(|(_, action)| action.clone()))
            .inner
    }
}

#[cfg(test)]
mod tests {
    use super::Blocks;
    use crate::core::quartz_domain::{LogicNode, QuartzAction, QuartzCondition, QuartzTargetRef};

    #[test]
    fn event_steps_round_trip_through_blocks() {
        let player = QuartzTargetRef::Name("player".to_owned());
        let push = QuartzAction::ApplyMomentum { target: player.clone(), mx: 1.0, my: 0.0 };
        let hide = QuartzAction::Hide { target: player };
        let steps = QuartzAction::Multi {
            actions: vec![
                push.clone(),
                QuartzAction::Conditional { condition: QuartzCondition::Always, if_true: Box::new(hide.clone()), if_false: None },
            ],
        };
        let nodes = Blocks::steps_of(Some(&steps));
        assert!(matches!(&nodes[1], LogicNode::Branch { then_nodes, else_nodes, .. } if then_nodes.len() == 1 && else_nodes.is_empty()));
        let json = |action: Option<QuartzAction>| serde_json::to_value(action).unwrap();
        assert_eq!(json(Blocks::to_action(&nodes)), json(Some(steps)));
        assert_eq!(json(Blocks::to_action(&Blocks::steps_of(Some(&push)))), json(Some(push)));
        assert!(Blocks::to_action(&[]).is_none());
    }
}
