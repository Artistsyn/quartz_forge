//! Starting points for a new project: a scene with the objects and controls a kind of game
//! always needs, so the first Play Live already moves something.

use crate::core::project::ProjectManifest;
use crate::core::quartz_domain::{
    ObjectTemplate, QuartzAction, QuartzEventBinding, QuartzEventKind, QuartzKeyModifiers, QuartzObjectBlueprint,
    QuartzObjectCollisionMode, QuartzTargetRef,
};

/// The size the template layouts are drawn at.
const STAGE: (f32, f32) = (1280.0, 720.0);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::app) enum ProjectTemplate {
    Empty,
    Platformer,
    TopDown,
    PhysicsSandbox,
}

impl ProjectTemplate {
    pub const ALL: [Self; 4] = [Self::Empty, Self::Platformer, Self::TopDown, Self::PhysicsSandbox];

    pub fn label(self) -> &'static str {
        match self {
            Self::Empty => "Empty",
            Self::Platformer => "Platformer",
            Self::TopDown => "Top-down",
            Self::PhysicsSandbox => "Physics sandbox",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Self::Empty => "One empty scene.",
            Self::Platformer => "A player with gravity, arrow keys to run, Space to jump, and a floor.",
            Self::TopDown => "A player that walks in four directions with the arrow keys, inside walls.",
            Self::PhysicsSandbox => "Crates and balls that fall, stack and roll on a floor.",
        }
    }

    /// Fill the first scene. Layouts are drawn on a 1280x720 stage and scaled, speeds and
    /// gravity with them, to the scene's own size.
    pub fn apply(self, manifest: &mut ProjectManifest) {
        let Some(scene) = manifest.scenes.first_mut() else { return };
        let (mut objects, mut events) = match self {
            Self::Empty => return,
            Self::Platformer => Self::platformer(),
            Self::TopDown => Self::top_down(),
            Self::PhysicsSandbox => (Self::sandbox(), Vec::new()),
        };
        let scale = (scene.canvas.virtual_width / STAGE.0).min(scene.canvas.virtual_height / STAGE.1);
        let offset = ((scene.canvas.virtual_width - STAGE.0 * scale) / 2.0, (scene.canvas.virtual_height - STAGE.1 * scale) / 2.0);
        for blueprint in &mut objects {
            [blueprint.x, blueprint.y] = [blueprint.x * scale + offset.0, blueprint.y * scale + offset.1];
            [blueprint.w, blueprint.h] = [blueprint.w * scale, blueprint.h * scale];
            blueprint.advanced.gravity *= scale;
        }
        for binding in &mut events {
            if let Some(QuartzAction::ApplyMomentum { mx, my, .. }) = &mut binding.action {
                (*mx, *my) = (*mx * scale, *my * scale);
            }
        }
        scene.objects = objects;
        scene.events = events;
    }

    fn platformer() -> (Vec<QuartzObjectBlueprint>, Vec<QuartzEventBinding>) {
        let mut player = Self::block("player", [120.0, 360.0, 64.0, 64.0], [255, 150, 60]);
        player.tags = vec!["player".to_owned()];
        player.advanced.gravity = 0.6;
        (player.advanced.resistance_x, player.advanced.resistance_y) = (0.85, 1.0);
        player.advanced.collision_mode = QuartzObjectCollisionMode::SolidRectangle;
        let floor = Self::solid("floor", [0.0, 620.0, 1280.0, 80.0]);
        let ledge = Self::solid("ledge", [520.0, 470.0, 260.0, 30.0]);
        let events = vec![
            Self::key_hold("event_run_left", "ArrowLeft", -1.2, 0.0),
            Self::key_hold("event_run_right", "ArrowRight", 1.2, 0.0),
            Self::key_press("event_jump", "Space", 0.0, -12.0),
        ];
        (vec![floor, ledge, player], events)
    }

    fn top_down() -> (Vec<QuartzObjectBlueprint>, Vec<QuartzEventBinding>) {
        let mut player = Self::block("player", [600.0, 320.0, 56.0, 56.0], [90, 200, 255]);
        player.template = ObjectTemplate::Circle;
        player.tags = vec!["player".to_owned()];
        (player.advanced.resistance_x, player.advanced.resistance_y) = (0.8, 0.8);
        player.advanced.collision_mode = QuartzObjectCollisionMode::SolidCircle;
        let walls = [
            ("wall_top", [0.0, 0.0, 1280.0, 30.0]),
            ("wall_bottom", [0.0, 690.0, 1280.0, 30.0]),
            ("wall_left", [0.0, 0.0, 30.0, 720.0]),
            ("wall_right", [1250.0, 0.0, 30.0, 720.0]),
        ];
        let mut objects: Vec<_> = walls.iter().map(|(id, rect)| Self::solid(id, *rect)).collect();
        objects.push(player);
        let events = vec![
            Self::key_hold("event_walk_left", "ArrowLeft", -1.0, 0.0),
            Self::key_hold("event_walk_right", "ArrowRight", 1.0, 0.0),
            Self::key_hold("event_walk_up", "ArrowUp", 0.0, -1.0),
            Self::key_hold("event_walk_down", "ArrowDown", 0.0, 1.0),
        ];
        (objects, events)
    }

    fn sandbox() -> Vec<QuartzObjectBlueprint> {
        let mut objects = vec![Self::solid("floor", [0.0, 640.0, 1280.0, 80.0])];
        for index in 0..6_u8 {
            let column = f32::from(index % 3);
            let row = f32::from(index / 3);
            let id = format!("crate_{}", index + 1);
            let mut body = Self::block(&id, [480.0 + column * 110.0, 200.0 - row * 120.0, 90.0, 90.0], [190, 140, 90]);
            if index % 2 == 1 {
                body.template = ObjectTemplate::Circle;
                body.advanced.collision_mode = QuartzObjectCollisionMode::SolidCircle;
            } else {
                body.advanced.collision_mode = QuartzObjectCollisionMode::SolidRectangle;
            }
            body.advanced.gravity = 0.5;
            (body.advanced.resistance_x, body.advanced.resistance_y) = (0.98, 1.0);
            objects.push(body);
        }
        objects
    }

    fn block(id: &str, rect: [f32; 4], color: [u8; 3]) -> QuartzObjectBlueprint {
        let mut blueprint = QuartzObjectBlueprint::new(id.to_owned(), id.to_owned());
        [blueprint.x, blueprint.y, blueprint.w, blueprint.h] = rect;
        blueprint.color_rgb = color;
        (blueprint.advanced.resistance_x, blueprint.advanced.resistance_y) = (1.0, 1.0);
        blueprint
    }

    fn solid(id: &str, rect: [f32; 4]) -> QuartzObjectBlueprint {
        let mut blueprint = Self::block(id, rect, [70, 78, 92]);
        blueprint.advanced.collision_mode = QuartzObjectCollisionMode::SolidRectangle;
        blueprint
    }

    fn key_hold(id: &str, key: &str, mx: f32, my: f32) -> QuartzEventBinding {
        let kind = QuartzEventKind::KeyHold { key: key.to_owned(), modifiers: QuartzKeyModifiers::default() };
        Self::player_push(id, kind, mx, my)
    }

    fn key_press(id: &str, key: &str, mx: f32, my: f32) -> QuartzEventBinding {
        let kind = QuartzEventKind::KeyPress { key: key.to_owned(), modifiers: QuartzKeyModifiers::default() };
        Self::player_push(id, kind, mx, my)
    }

    fn player_push(id: &str, kind: QuartzEventKind, mx: f32, my: f32) -> QuartzEventBinding {
        let player = QuartzTargetRef::Name("player".to_owned());
        let name = id.trim_start_matches("event_").replace('_', " ");
        let mut binding = QuartzEventBinding::new(id.to_owned(), name, player.clone());
        binding.kind = kind;
        binding.action = Some(QuartzAction::ApplyMomentum { target: player, mx, my });
        binding
    }
}
