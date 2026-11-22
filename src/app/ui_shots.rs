//! The editor rendered headlessly (egui_kittest + wgpu) and driven by clicks and keys the way a
//! person uses it. Every run checks the screens build and the interactions do what they say;
//! with `QF_UI_SHOTS=<folder>` set it also writes each screen as a PNG to look at:
//! `QF_UI_SHOTS=/tmp/shots cargo test --lib ui_shots -- --test-threads=1`.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use egui::{Key, Modifiers};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;

use super::QuartzForgeApp;
use super::shell::{CenterTab, DockTab, InspectorTab, RecentProjects, Shell};
use crate::core::project::ProjectManifest;
use crate::core::quartz_domain::{
    LogicNode, LogicTree, ObjectVisualAssetMode, QuartzAction, QuartzEventBinding, QuartzEventKind, QuartzKeyModifiers,
    QuartzObjectBlueprint, QuartzObjectCollisionMode, QuartzTargetRef,
};
use crate::core::store::ProjectStore;
use crate::services::gaps::{GapArea, GapBook, GapProposal, GapStatus};

const WINDOW: (f32, f32) = (1440.0, 900.0);

/// One editor window under test, with its own scratch folder for projects, gaps and recents.
struct UiShots {
    harness: Harness<'static, QuartzForgeApp>,
    scratch: PathBuf,
}

impl UiShots {
    fn new(test: &str) -> Result<Self> {
        let scratch = std::env::temp_dir().join(format!("qf_ui_{test}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&scratch);
        std::fs::create_dir_all(&scratch)?;
        let mut app = QuartzForgeApp::default();
        app.shell = Shell::new(RecentProjects::at(scratch.join("recent.json")), GapBook::at(scratch.join("gaps")));
        app.shell.welcome.location = scratch.join("games");
        let harness = Harness::builder().with_size(egui::vec2(WINDOW.0, WINDOW.1)).build_eframe(move |_cc| app);
        let mut shots = Self { harness, scratch };
        shots.settle();
        Ok(shots)
    }

    fn with_project(test: &str) -> Result<Self> {
        let mut shots = Self::new(test)?;
        let root = shots.sample_project("sky_courier")?;
        shots.app().open_project_at(&root);
        shots.settle();
        Ok(shots)
    }

    fn app(&mut self) -> &mut QuartzForgeApp {
        self.harness.state_mut()
    }

    fn settle(&mut self) {
        self.harness.run_steps(4);
    }

    fn shot(&mut self, name: &str) -> Result<()> {
        self.settle();
        let Some(dir) = std::env::var_os("QF_UI_SHOTS") else { return Ok(()) };
        let dir = PathBuf::from(dir);
        std::fs::create_dir_all(&dir)?;
        let image = self.harness.render().map_err(|error| anyhow::anyhow!("render {name}: {error}"))?;
        image.save(dir.join(format!("{name}.png"))).context("save shot")
    }

    fn click(&mut self, label: &str) {
        self.harness.get_by_label(label).click();
        self.settle();
    }

    fn click_containing(&mut self, label: &str) {
        self.harness.get_by_label_contains(label).click();
        self.settle();
    }

    /// Select an object by its hierarchy row (glyph, two spaces, name).
    fn select_object(&mut self, name: &str) {
        self.click_containing(&format!("  {name}"));
    }

    fn keys(&mut self, modifiers: Modifiers, key: Key) {
        self.harness.key_press_modifiers(modifiers, key);
        self.settle();
    }

    fn object_x(&mut self, index: usize) -> f32 {
        let app = self.app();
        app.project_state.manifest.scenes[app.project_state.active_scene_index].objects[index].x
    }

    fn block(id: &str, rect: [f32; 4]) -> QuartzObjectBlueprint {
        let mut blueprint = QuartzObjectBlueprint::new(id.to_owned(), id.to_owned());
        [blueprint.x, blueprint.y, blueprint.w, blueprint.h] = rect;
        blueprint
    }

    /// A small platformer with images, an event and a logic tree: realistic weight.
    fn sample_project(&self, name: &str) -> Result<PathBuf> {
        let root = self.scratch.join(name);
        let assets = Path::new(env!("CARGO_MANIFEST_DIR")).join("../ball_swing_game/assets");
        std::fs::create_dir_all(root.join("assets"))?;
        for file in ["rounded_rectangle.png", "oxygencanister.png"] {
            std::fs::copy(assets.join(file), root.join("assets").join(file))?;
        }
        let mut manifest = ProjectManifest::new("Sky Courier".to_owned());
        let scene = &mut manifest.scenes[0];
        let mut player = Self::block("player", [600.0, 300.0, 90.0, 90.0]);
        player.tags = vec!["player".to_owned()];
        player.color_rgb = [255, 140, 40];
        player.advanced.collision_mode = QuartzObjectCollisionMode::SolidCircle;
        let mut floor = Self::block("floor", [150.0, 520.0, 900.0, 70.0]);
        floor.visual_asset_mode = ObjectVisualAssetMode::StaticImage;
        floor.visual_asset_path = "assets/rounded_rectangle.png".to_owned();
        let mut canister = Self::block("canister", [250.0, 260.0, 90.0, 140.0]);
        canister.visual_asset_mode = ObjectVisualAssetMode::StaticImage;
        canister.visual_asset_path = "assets/oxygencanister.png".to_owned();
        let mut bullet = Self::block("bullet", [0.0, 0.0, 16.0, 16.0]);
        bullet.spawn_only = true;
        scene.objects = vec![floor, canister, player, bullet];
        let target = |id: &str| QuartzTargetRef::Name(id.to_owned());
        let mut spin = LogicTree::new("tree_spin".to_owned(), "spin canister".to_owned());
        spin.nodes = vec![LogicNode::Action(QuartzAction::AddRotation { target: target("canister"), deg: 2.0 })];
        scene.logic_trees = vec![spin];
        let mut step = QuartzEventBinding::new("event_step".to_owned(), "step right".to_owned(), target("player"));
        step.kind = QuartzEventKind::KeyHold { key: "ArrowRight".to_owned(), modifiers: QuartzKeyModifiers::default() };
        step.action = Some(QuartzAction::ApplyMomentum { target: target("player"), mx: 1.5, my: 0.0 });
        scene.events = vec![step];
        ProjectStore::new(&root).save(&manifest)?;
        Ok(root)
    }
}

impl Drop for UiShots {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.scratch);
    }
}

#[test]
fn ui_shots_welcome_creates_a_project_from_a_template() -> Result<()> {
    let mut shots = UiShots::new("welcome")?;
    let recent = shots.sample_project("older_game")?;
    shots.app().shell.recent.note_opened("Older Game", &recent);
    shots.shot("01_welcome")?;
    shots.click("Top-down");
    shots.click("Create Project");
    let app = shots.app();
    let root = app.project_root.clone().expect("the project opened");
    assert!(root.starts_with(shots.scratch.join("games")), "{}", root.display());
    assert!(root.join("data/scenes").is_dir(), "data files written");
    let app = shots.app();
    assert_eq!(app.project_state.manifest.scenes[0].objects.len(), 5, "four walls and a player");
    assert_eq!(app.shell.recent.entries[0].path, root);
    shots.shot("02_created_top_down")?;
    Ok(())
}

#[test]
fn ui_shots_workspace_panels_and_tabs() -> Result<()> {
    let mut shots = UiShots::with_project("workspace")?;
    shots.shot("03_workspace")?;
    shots.select_object("canister");
    assert_eq!(shots.app().selected_object_index, 1);
    shots.shot("04_object_selected")?;
    shots.click("Scene Settings");
    assert_eq!(shots.app().shell.tabs.inspector, InspectorTab::Scene);
    shots.shot("05_scene_settings")?;
    for (label, tab) in [("Code Blocks", DockTab::CodeBlocks), ("Lighting", DockTab::Lighting), ("Output", DockTab::Output), ("Events & Logic", DockTab::Events)] {
        shots.click(label);
        assert_eq!(shots.app().shell.tabs.dock, tab);
        shots.shot(&format!("06_dock_{}", label.to_lowercase().replace([' ', '&'], "_")))?;
    }
    shots.click("Camera View");
    assert_eq!(shots.app().shell.tabs.center, CenterTab::Camera);
    shots.shot("07_camera_view")?;
    shots.click("Code");
    assert!(shots.app().quartz_preview.contains("fn "), "code view shows generated code");
    shots.shot("08_code_view")?;
    shots.click("Edit");
    shots.shot("09_edit_menu")?;
    Ok(())
}

#[test]
fn ui_shots_undo_redo_duplicate_delete() -> Result<()> {
    let mut shots = UiShots::with_project("undo")?;
    let before = shots.object_x(2);
    shots.app().project_state.manifest.scenes[0].objects[2].x = before + 300.0;
    shots.harness.hover_at(egui::pos2(700.0, 400.0));
    shots.settle();
    shots.keys(Modifiers::COMMAND, Key::Z);
    assert!((shots.object_x(2) - before).abs() < 0.01, "undo restores x");
    shots.keys(Modifiers::COMMAND | Modifiers::SHIFT, Key::Z);
    assert!((shots.object_x(2) - before - 300.0).abs() < 0.01, "redo reapplies x");
    shots.select_object("player");
    shots.keys(Modifiers::COMMAND, Key::D);
    assert_eq!(shots.app().project_state.manifest.scenes[0].objects.len(), 5, "duplicated");
    shots.keys(Modifiers::NONE, Key::Delete);
    assert_eq!(shots.app().project_state.manifest.scenes[0].objects.len(), 4, "deleted");
    shots.keys(Modifiers::COMMAND, Key::Z);
    assert_eq!(shots.app().project_state.manifest.scenes[0].objects.len(), 5, "undo brings the deleted copy back");
    Ok(())
}

#[test]
fn ui_shots_command_palette_runs_a_command() -> Result<()> {
    let mut shots = UiShots::with_project("palette")?;
    shots.keys(Modifiers::COMMAND, Key::K);
    shots.harness.event(egui::Event::Text("camera".to_owned()));
    shots.shot("10_command_palette")?;
    shots.keys(Modifiers::NONE, Key::Enter);
    assert_eq!(shots.app().shell.tabs.center, CenterTab::Camera);
    Ok(())
}

#[test]
fn ui_shots_gap_report_is_approved_in_the_editor() -> Result<()> {
    let mut shots = UiShots::with_project("gaps")?;
    let book = GapBook::at(shots.scratch.join("gaps"));
    let filed = book
        .propose(GapProposal {
            title: "Falling-sand cell world".to_owned(),
            area: GapArea::Engine,
            game: "Mine Crawler".to_owned(),
            need: "Digging and flowing sand need a per-pixel material world the objects collide with.".to_owned(),
            proposal: "Add crystalline::sand: 64x64 chunks with dirty rectangles, checkerboard threads, GPU upload of dirty chunks.".to_owned(),
            effort: "large (weeks)".to_owned(),
        })
        .unwrap();
    shots.settle();
    shots.app().shell.gaps.show();
    shots.shot("11_gap_reports")?;
    shots.click("Approve");
    let decided = book.list().unwrap().into_iter().find(|report| report.id == filed.id).unwrap();
    assert_eq!(decided.status, GapStatus::Approved);
    Ok(())
}

#[test]
fn ui_shots_new_project_dialog_over_an_open_project() -> Result<()> {
    let mut shots = UiShots::with_project("dialog")?;
    shots.keys(Modifiers::COMMAND, Key::N);
    assert!(shots.app().shell.welcome.dialog_open);
    shots.shot("12_new_project_dialog")?;
    shots.click("Cancel");
    assert!(!shots.app().shell.welcome.dialog_open);
    Ok(())
}

#[test]
fn ui_shots_effects_tab_makes_a_preset_plays_it_and_puts_it_on_an_object() -> Result<()> {
    let mut shots = UiShots::with_project("effects")?;
    shots.click("Effects");
    assert_eq!(shots.app().shell.tabs.dock, DockTab::Effects);
    shots.shot("16_effects_empty")?;
    shots.click("Preset ⏵");
    shots.shot("16b_effects_presets")?;
    shots.click("fire");
    assert_eq!(shots.app().effects.selected.as_deref(), Some("fire"));
    assert_eq!(shots.app().project_state.manifest.particle_effects.len(), 1);
    shots.harness.run_steps(20);
    assert!(shots.app().effects.preview.age > 0.1, "the preview plays: {}", shots.app().effects.preview.age);
    shots.shot("17_effects_fire")?;
    shots.click("Release");
    shots.click("Flight");
    shots.click("Look");
    shots.click("Over life");
    shots.click("+ Key over life");
    assert_eq!(shots.app().project_state.manifest.particle_effects[0].look.keys.len(), 1);
    shots.harness.run_steps(10);
    shots.shot("17b_effects_key_over_life")?;
    shots.select_object("canister");
    shots.click("⬇");
    shots.click("Appearance");
    shots.click("Particle Effects");
    shots.click("+ Play an effect on this object");
    let blueprint = &shots.app().project_state.manifest.scenes[0].objects[1];
    assert_eq!(blueprint.particles.first().map(|attached| attached.effect.as_str()), Some("fire"));
    shots.shot("18_object_particles")?;
    let plume = crate::core::object_effect::ObjectEffectSpec { kind: crate::core::object_effect::EffectKind::ThrustPlume, ..Default::default() };
    shots.app().project_state.manifest.scenes[0].objects[1].effect = Some(plume);
    shots.click("Transform");
    shots.click("Particle Effects");
    shots.click("Appearance");
    shots.click("Play the effect here");
    assert!(shots.app().effects.preview.shader_open);
    shots.harness.run_steps(10);
    shots.shot("19_shader_effect_preview")?;
    shots.harness.get_by_label_contains("plain screens").scroll_to_me();
    shots.harness.run_steps(4);
    shots.shot("19b_shader_effect_cost")?;
    shots.click("Play the effect here");
    shots.harness.run_steps(4);
    assert!(shots.app().effects.scene.is_drawing() || shots.app().effects.scene.failure().is_some());
    shots.shot("19c_scene_view_effects")?;
    Ok(())
}

#[test]
fn ui_shots_shaders_view_designs_a_layer_stack_and_puts_it_on_an_object() -> Result<()> {
    let mut shots = UiShots::with_project("shaders")?;
    shots.click("Effects");
    shots.click("Shaders");
    shots.shot("20_shaders_empty")?;
    shots.click("New ⏵");
    shots.click("Layers");
    assert_eq!(shots.app().effects.shaders.selected.as_deref(), Some("shader"));
    shots.harness.run_steps(10);
    shots.shot("20b_shader_layers")?;
    shots.click("+ Layer");
    shots.click("Sparkle");
    let layers = |shots: &mut UiShots| match &shots.app().project_state.manifest.shaders[0].authoring {
        crate::core::shader_asset::ShaderAuthoring::Layers { stack } => stack.layers.len(),
        crate::core::shader_asset::ShaderAuthoring::Wgsl => 0,
    };
    assert_eq!(layers(&mut shots), 3);
    assert!(shots.app().project_state.dirty);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(90);
    while shots.app().project_state.manifest.shaders[0].current_weight().is_none() && shots.app().effects.shaders.measure_failure().is_none() && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(100));
        shots.harness.run_steps(2);
    }
    assert!(shots.app().effects.shaders.thumbnails_shown() == 3 || shots.app().effects.shaders.measure_failure().is_some());
    shots.shot("20c_shader_sparkle_measured")?;
    let choice = shots.app().project_state.manifest.shaders[0].choice();
    let spec = crate::core::object_effect::ObjectEffectSpec { project: Some(choice), ..Default::default() };
    shots.app().project_state.manifest.scenes[0].objects[1].effect = Some(spec.clone());
    let scene_size = (1280.0, 720.0);
    let findings = spec.check((120.0, 120.0), scene_size, &shots.app().project_state.manifest.shaders);
    assert!(findings.iter().all(|finding| finding.level != crate::core::particle_effect::FindingLevel::Block), "{findings:?}");
    shots.click("Particles");
    shots.select_object("canister");
    shots.click("⬇");
    shots.click("Transform");
    shots.click("Play the effect here");
    shots.harness.run_steps(10);
    shots.shot("21_object_project_shader")?;
    Ok(())
}

#[test]
fn ui_shots_rules_editor_adds_a_step_by_menu() -> Result<()> {
    let mut shots = UiShots::with_project("rules")?;
    shots.click_containing("step right");
    shots.shot("13_rules_event")?;
    shots.click("+ Add step");
    shots.click("Camera ⏵");
    shots.shot("14_add_step_menu")?;
    shots.click("Shake");
    let event = &shots.app().project_state.manifest.scenes[0].events[0];
    let steps = serde_json::to_value(&event.action)?;
    assert!(steps.pointer("/Multi/actions/1/CameraShake").is_some(), "{steps}");
    shots.click("Push (add speed)");
    shots.shot("14b_step_open")?;
    shots.click_containing("spin canister");
    shots.shot("15_rules_every_frame")?;
    Ok(())
}
