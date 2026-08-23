//! Runnable-code verification: generate a representative game project from a
//! forge manifest and hand it to the REAL engine toolchain.
//!
//! This binary writes `<workspace>/forge_verify_game` — a complete project
//! exercising the authoring surface end to end: plugin registration, a pooled
//! spawn-only template, world objects with physics, events covering the
//! action-parity pass (Teleport, momentum, PlaySound, SetText, CameraShake,
//! conditionals, vars), and — in this SYNFUL fork — the real-time lighting +
//! GPU post-fx surface (every LightType, every LightEffect, attachment, a
//! disabled/shadowless light, night-mode post-fx + bloom, and per-object
//! unlit/casts_shadow flags). It deliberately does NOT compile the result
//! itself:
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
    LightEffectSpec, LightKindSpec, LightSpec, LightingSpec, PostFxMode, PostFxSpec,
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
        // SYNFUL: terrain occludes light; it is also 3840 wide, and objects that
        // large get tinted uniformly from their center, so mark it unlit too.
        ground.casts_shadow = true;
        ground.unlit = true;
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

        // ── SYNFUL lighting: every LightType and every LightEffect ───────────
        // The point is to prove each emitted variant compiles against the real
        // synful engine, not to make a scene that looks good.
        scene.lighting = LightingSpec {
            enabled: true,
            ambient_color: [10, 10, 25, 255],
            ambient_strength: 0.06,
            max_lights: 64,
            lights: vec![
                // Attached, shadow-casting, flickering torch (follows the player).
                LightSpec {
                    id: "torch".to_owned(),
                    kind: LightKindSpec::Point,
                    x: 400.0,
                    y: 300.0,
                    color: [255, 180, 80, 255],
                    radius: 300.0,
                    intensity: 0.8,
                    enabled: true,
                    casts_shadows: true,
                    effect: LightEffectSpec::Flicker { base_intensity: 0.8, variance: 0.2 },
                    attach_object: "player".to_owned(),
                    attach_offset_x: 0.0,
                    attach_offset_y: -16.0,
                },
                // Spot + pulse + shadows OFF (exercises .with_shadows(false)).
                LightSpec {
                    id: "spot".to_owned(),
                    kind: LightKindSpec::Spot { direction: 90.0, cone_angle: 45.0 },
                    x: 800.0,
                    y: 200.0,
                    color: [255, 255, 240, 255],
                    radius: 500.0,
                    intensity: 1.0,
                    enabled: true,
                    casts_shadows: false,
                    effect: LightEffectSpec::Pulse {
                        min_intensity: 0.4,
                        max_intensity: 1.0,
                        speed: 2.0,
                    },
                    ..LightSpec::new("spot")
                },
                // Directional + disabled (exercises the struct-update path for
                // both light_type and enabled: false).
                LightSpec {
                    id: "sun".to_owned(),
                    kind: LightKindSpec::Directional { dx: 0.3, dy: -1.0 },
                    x: 0.0,
                    y: 0.0,
                    color: [255, 248, 220, 255],
                    radius: 4000.0,
                    intensity: 0.6,
                    enabled: false,
                    casts_shadows: true,
                    effect: LightEffectSpec::ColorCycle {
                        colors: vec![[255, 200, 150, 255], [150, 200, 255, 255]],
                        speed: 0.5,
                    },
                    ..LightSpec::new("sun")
                },
                LightSpec {
                    id: "fade_in".to_owned(),
                    effect: LightEffectSpec::FadeIn { target_intensity: 1.0, duration: 2.0 },
                    ..LightSpec::new("fade_in")
                },
                LightSpec {
                    id: "fade_out".to_owned(),
                    effect: LightEffectSpec::FadeOut { duration: 1.5 },
                    ..LightSpec::new("fade_out")
                },
            ],
        };

        // ── SYNFUL post-fx: bloom + night mode (bloom is a separate pass) ────
        scene.post_fx = PostFxSpec {
            bloom_enabled: true,
            bloom_threshold: 0.8,
            bloom_strength: 0.4,
            mode: PostFxMode::NightMode {
                bloom_threshold: 0.75,
                bloom_strength: 0.5,
                vignette_strength: 0.6,
                vignette_radius: 0.7,
                vignette_softness: 0.3,
                ca_intensity: 1.5,
            },
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
