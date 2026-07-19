//! Runnable-code verification: generate a representative game project from a
//! forge manifest and hand it to the REAL engine toolchain.
//!
//! This binary writes `<workspace>/forge_verify_game` — a complete project
//! exercising the authoring surface end to end: plugin registration, a pooled
//! spawn-only template, world objects with physics, and events covering the
//! action-parity pass (Teleport, momentum, PlaySound, SetText, CameraShake,
//! conditionals, vars). It deliberately does NOT compile the result itself:
//!
//! ```powershell
//! cargo run --manifest-path quartz_forge/Cargo.toml --bin qf_verify_generated
//! cargo check --manifest-path forge_verify_game/Cargo.toml
//! ```
//!
//! The second command is the actual verdict — generated code either builds
//! against quartz/crystalline or the generator is wrong. Nothing in the
//! editor matters if this gate is red.
use anyhow::Result;
use quartz_forge::core::project::{
    BackgroundLayerSpec, BackgroundResizeFilter, BackgroundSpec, CameraSpec, NamedBackground,
    PluginRegistration,
    PoolBlueprint,
};
use quartz_forge::core::quartz_domain::{
    QuartzAction, QuartzCondition, QuartzEventBinding, QuartzEventKind, QuartzExpr,
    QuartzExprKind, QuartzKeyModifiers, QuartzLocationRef, QuartzMathOp,
    QuartzObjectBlueprint, QuartzTargetRef, CompareOp,
};
use quartz_forge::services::{persistence, project_sync};

fn player() -> QuartzTargetRef {
    QuartzTargetRef::Name("player".to_owned())
}

fn f32_expr(value: f32) -> QuartzExpr {
    QuartzExpr {
        kind: QuartzExprKind::F32,
        raw: format!("{value:.1}"),
    }
}

fn main() -> Result<()> {
    let workspace = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("workspace root")
        .to_path_buf();
    let root = workspace.join("forge_verify_game");
    if root.exists() {
        std::fs::remove_dir_all(&root)?;
    }

    let mut state = persistence::create_new_project("forge_verify_game".to_owned(), &root)?;

    {
        let scene = &mut state.manifest.scenes[0];
        scene.source_file = "src/scenes/main_scene.rs".to_owned();

        // ── Plugin registration (the silent-no-op fix, exercised) ────────────
        scene
            .required_plugins
            .push(PluginRegistration::new("TerrainCollisionPlugin"));

        // ── Objects ──────────────────────────────────────────────────────────
        let mut player_obj =
            QuartzObjectBlueprint::new("player".to_owned(), "player".to_owned());
        player_obj.x = 400.0;
        player_obj.y = 300.0;
        player_obj.w = 64.0;
        player_obj.h = 64.0;
        player_obj.layer = 5;
        player_obj.tags = vec!["player".to_owned()];
        player_obj.advanced.gravity = 900.0;
        player_obj.advanced.resistance_x = 2.0;
        player_obj.advanced.resistance_y = 0.0;
        scene.objects.push(player_obj);

        let mut ground =
            QuartzObjectBlueprint::new("ground".to_owned(), "ground".to_owned());
        ground.x = 0.0;
        ground.y = 2000.0;
        ground.w = 3840.0;
        ground.h = 160.0;
        ground.layer = 1;
        ground.tags = vec!["terrain".to_owned()];
        scene.objects.push(ground);

        // Pooled bullet template: spawn-only, manually controlled (gravity 0).
        let mut bullet =
            QuartzObjectBlueprint::new("bullet".to_owned(), "bullet".to_owned());
        bullet.x = -6000.0;
        bullet.y = -6000.0;
        bullet.w = 12.0;
        bullet.h = 12.0;
        bullet.layer = 6;
        bullet.spawn_only = true;
        bullet.tags = vec!["bullet".to_owned()];
        bullet.advanced.gravity = 0.0;
        scene.objects.push(bullet);

        scene.pools.push(PoolBlueprint {
            pool_tag: "bullets".to_owned(),
            template_object_id: "bullet".to_owned(),
            count: 32,
        });

        // ── Camera authoring: follow player, snap to 1.5x at start ───────────
        scene.camera = CameraSpec {
            follow_enabled: true,
            follow_target: player(),
            initial_zoom: 1.5,
            smooth_initial_zoom: false,
        };

        // ── Background: composited procedural layers (renders as unlit obj) ───
        scene.background = BackgroundSpec {
            enabled: true,
            object_id: "background".to_owned(),
            render_layer: -100,
            camera_pinned: true,
            tint: [255, 255, 255],
            // Plugin-cache mode: proves the BackgroundPlugin path (disk cache +
            // current_image pull) links against the engine. Direct mode was
            // already proven earlier this session.
            use_plugin_cache: true,
            cache_dir: "assets/bg_cache".to_owned(),
            background_key: "main".to_owned(),
            active_key: "main".to_owned(),
            // Per-frame pull + a second named background prove crossfade
            // transitions: RunPlugin("background","transition:main,dusk,1.5").
            per_frame_pull: true,
            backgrounds: vec![NamedBackground {
                key: "dusk".to_owned(),
                tint: [255, 200, 160],
                layers: vec![
                    BackgroundLayerSpec::GradientVertical {
                        top: [40, 20, 60],
                        bottom: [10, 4, 20],
                    },
                    BackgroundLayerSpec::Nebula { color: [120, 60, 40], density: 0.3, seed: 0x99 },
                    // Image layer: proves include_bytes! + the ResizeFilter
                    // re-export compile against the engine.
                    BackgroundLayerSpec::Image {
                        asset_path: "assets/bg_layer.png".to_owned(),
                        filter: BackgroundResizeFilter::Lanczos3,
                    },
                ],
            }],
            layers: vec![
                BackgroundLayerSpec::GradientVertical {
                    top: [8, 26, 74],
                    bottom: [2, 4, 16],
                },
                BackgroundLayerSpec::Starfield {
                    density: 300,
                    seed: 0xCAFE_BABE,
                    size_min: 0,
                    size_max: 1,
                    brightness_min: 100,
                    brightness_max: 255,
                    vertical_fade: Some(200),
                },
                BackgroundLayerSpec::Nebula {
                    color: [80, 40, 120],
                    density: 0.4,
                    seed: 0x1234,
                },
            ],
        };

        // ── Events exercising the parity-pass actions ────────────────────────
        let mut jump = QuartzEventBinding::new(
            "evt_jump".to_owned(),
            "jump".to_owned(),
            player(),
        );
        jump.kind = QuartzEventKind::KeyPress {
            key: "Space".to_owned(),
            modifiers: QuartzKeyModifiers::default(),
        };
        jump.action = Some(QuartzAction::Multi {
            actions: vec![
                QuartzAction::ApplyMomentum {
                    target: player(),
                    mx: 0.0,
                    my: -650.0,
                },
                QuartzAction::PlaySound {
                    path: "assets/jump.wav".to_owned(),
                    volume: 0.8,
                    looping: false,
                },
            ],
        });
        scene.events.push(jump);

        let mut dash = QuartzEventBinding::new(
            "evt_dash".to_owned(),
            "dash".to_owned(),
            player(),
        );
        dash.kind = QuartzEventKind::KeyHold {
            key: "D".to_owned(),
            modifiers: QuartzKeyModifiers::default(),
        };
        dash.action = Some(QuartzAction::SetMomentum {
            target: player(),
            mx: 420.0,
            my: 0.0,
        });
        scene.events.push(dash);

        let mut land = QuartzEventBinding::new(
            "evt_land".to_owned(),
            "land".to_owned(),
            player(),
        );
        land.kind = QuartzEventKind::Collision;
        land.action = Some(QuartzAction::Conditional {
            condition: QuartzCondition::SpeedAbove {
                target: player(),
                value: 500.0,
            },
            if_true: Box::new(QuartzAction::Multi {
                actions: vec![
                    QuartzAction::CameraShake {
                        intensity: 4.0,
                        duration_s: 0.35,
                    },
                    QuartzAction::AddTag {
                        target: player(),
                        tag: "hard_landing".to_owned(),
                    },
                ],
            }),
            if_false: Some(Box::new(QuartzAction::RemoveTag {
                target: player(),
                tag: "hard_landing".to_owned(),
            })),
        });
        scene.events.push(land);

        let mut score_tick = QuartzEventBinding::new(
            "evt_score".to_owned(),
            "score_tick".to_owned(),
            player(),
        );
        score_tick.kind = QuartzEventKind::Tick;
        score_tick.action = Some(QuartzAction::Conditional {
            condition: QuartzCondition::Compare {
                left: QuartzExpr {
                    kind: QuartzExprKind::Var,
                    raw: "score".to_owned(),
                },
                op: CompareOp::Lt,
                right: f32_expr(9999.0),
            },
            if_true: Box::new(QuartzAction::ModVar {
                name: "score".to_owned(),
                op: QuartzMathOp::Add,
                operand: f32_expr(1.0),
            }),
            if_false: None,
        });
        scene.events.push(score_tick);

        let mut respawn = QuartzEventBinding::new(
            "evt_respawn".to_owned(),
            "respawn".to_owned(),
            player(),
        );
        respawn.kind = QuartzEventKind::BoundaryCollision;
        respawn.action = Some(QuartzAction::Multi {
            actions: vec![
                QuartzAction::Teleport {
                    target: player(),
                    location: QuartzLocationRef::At { x: 400.0, y: 300.0 },
                },
                QuartzAction::SetMomentum {
                    target: player(),
                    mx: 0.0,
                    my: 0.0,
                },
                QuartzAction::SetVar {
                    name: "score".to_owned(),
                    value: f32_expr(0.0),
                },
            ],
        });
        scene.events.push(respawn);

        // Background crossfade wired exactly as the event builder's
        // "Background transition" button produces it. Must emit
        // Action::RunPlugin (on_action), NOT PluginCall (on_call) — the
        // background plugin only implements on_action.
        let mut bg_switch = QuartzEventBinding::new(
            "evt_bg_transition".to_owned(),
            "background_crossfade".to_owned(),
            player(),
        );
        bg_switch.kind = QuartzEventKind::KeyPress {
            key: "B".to_owned(),
            modifiers: QuartzKeyModifiers::default(),
        };
        bg_switch.action = Some(QuartzAction::RunPlugin {
            name: "background".to_owned(),
            data: "transition:main,dusk,1.5".to_owned(),
        });
        scene.events.push(bg_switch);
    }

    // The Image background layer emits include_bytes!, so the asset must exist
    // on disk before the generated crate is compiled.
    {
        let assets = root.join("assets");
        std::fs::create_dir_all(&assets)?;
        let mut img = image::RgbaImage::new(8, 8);
        for (x, y, px) in img.enumerate_pixels_mut() {
            *px = image::Rgba([(x * 32) as u8, (y * 32) as u8, 200, 255]);
        }
        img.save(assets.join("bg_layer.png"))?;
    }

    persistence::save_project(&mut state, &root)?;
    persistence::ensure_runtime_scaffold(&state, &root)?;
    project_sync::write_generated_files_for_scene(&state, &root, 0)?;

    println!("generated: {}", root.display());
    println!("verdict:   cargo check --manifest-path forge_verify_game/Cargo.toml");
    Ok(())
}
