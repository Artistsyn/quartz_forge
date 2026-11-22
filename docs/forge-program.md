# Quartz Forge program (from 2026-11-21)

The goal: any game idea can be designed in Quartz Forge, by a person in a professional editor
and by an agent through the MCP tools, on the same files, without losing anything either way.

## Done

- **Data-first projects.** `project.qforge.json` + `data/scenes/<scene>/{scene.json, objects/,
  events/, logic/}`, one part per file (`quartz_project::store::ProjectStore`).
- **Live play.** `forge_player <project>` plays the data and hot-reloads edits in about a second
  (`quartz_project_runtime`); the editor's Play Live button runs it and saves edits as you make them;
  the editor reloads files changed outside it.
- **Lossless round trip.** Every action, condition, branch, spawn and declared variable goes
  data → generated Rust → import back unchanged (`every_action_and_condition_survives_codegen_and_import`);
  the generated code for all of them compiles (`qf_verify_generated`).
- **Agent tools.** `qf_project_data_check`, `qf_project_entity`, `qf_project_examples`.
- **Games in their own folders.** New games default to `<engine parent>/games/<name>`; engine
  crates found by relative path; one shared build folder (`games/.target`); moved games repaired
  on save (`services/engine_location.rs`).
- **Gap reports.** `qf_gap_propose` / `qf_gap_list` / `qf_gap_mark_done`; Tools > Gap Reports
  (and the toolbar badge) to approve or reject with a note (`services/gaps.rs`, `app/shell/gap_review.rs`).
- **Editor shell.** Welcome screen (recent projects, templates: Empty, Platformer, Top-down,
  Physics sandbox), menu bar, toolbar with Play Live, docked Scenes/Hierarchy, Inspector
  (Object, Scene Settings) in labelled sections with an image gallery, scene view with grid,
  snap, Fit (F) and saved views, bottom panel (Events & Logic, Code Blocks, Background,
  Lighting, Files, Output), status bar, undo/redo for every edit (view pan/zoom excluded),
  command palette (Cmd+K), shortcuts, no windows over the scene view by default (`app/shell/`).
- **Rules editor.** Events and every-frame rules as "When … Do …": triggers in plain words,
  press-a-key picker, object/tag pickers, steps as blocks (If/Else nesting, move, duplicate,
  delete) added from a categorised menu of every action (`app/rules/`).
- **UI test harness.** `src/app/ui_shots.rs` renders the real editor headlessly (egui_kittest)
  and clicks through it; `QF_UI_SHOTS=<dir>` writes the screens as PNGs.

## Phases

1. **Foundations**
   - Every game in its own folder anywhere (default `<engine parent>/games/<name>`), engine found
     by relative path, one shared build folder for all games.
   - Gap reports: an agent that finds the engine or editor missing something a game needs files
     a gap with a proposed implementation (`qf_gap_propose`); the user approves or rejects it in
     the editor (Tools > Gap Reports). Nothing is built from a gap before approval.
2. **Professional editor** (each screen checked by a headless shot and click test)
   - Welcome screen: recent projects, new project from a template.
   - Menu bar (File, Edit, View, Scene, Play, Tools, Help) and a toolbar with Play Live.
   - Docked layout: Scenes and Hierarchy left, Viewport centre, Inspector right, bottom tabs for
     Assets, Events & Logic, Animation, Problems. No windows open over the canvas by default.
   - Inspector in collapsible sections (Transform, Appearance, Physics, Collision, Lighting,
     Effect, Tags) with labelled number fields, colour pickers and asset pickers with thumbnails.
   - Viewport: select, move, rotate and scale handles, multi-select, snapping, framing.
   - Undo and redo for every edit; keyboard shortcuts; a command palette.
   - Developer operations (reimport, round-trip check) move to Tools > Advanced.
3. **Animation authoring**: clips of keyframed tracks (position, rotation, scale, tint, sprite
   frame, any number field) and sprite-sheet flipbooks; an engine clip player; a timeline
   editor with a curve view; live in the Forge Player; round-trips through codegen.
4. **Rules in game building**: generated code follows the ssnebula/ephemeris rules (scene code
   in types, not free functions); every game gets an `ephemeris.toml`; the Problems panel shows
   rule findings for custom code blocks as you type; export runs the check.
5. **Falling-sand physics** (Noita-style), in crystalline (`crystalline::sand`).
   Done: cell world in 64x64 chunks with per-chunk dirty rectangles (a settled world costs
   nothing), powders/liquids/gases/solids with density displacement, materials and reactions
   as data (`MaterialTable::standard`: sand, water, oil, stone, wood, embers, fire, smoke, steam,
   lava, acid), four-pass checkerboard steps across threads with results identical to one
   thread, explosions, changed-region reporting. Measured 1024x576 busy: 8.6 ms/step on one
   thread, 2.8 ms on 10 (`examples/sand_bench.rs`). Next: a live texture in wgpu_canvas that
   takes partial uploads of the changed regions, a quartz drawable for the world, objects
   colliding with it (`SandWorld::is_solid`), then pixel rigid bodies.
   The full plan:
   a. Cell world in 64×64 chunks with sleeping and dirty rectangles; powders, liquids, gases,
      solids; four-pass checkerboard updates across threads; only dirty chunks uploaded to a GPU
      texture.
   b. Materials as data: density, flow, temperature, burning, melting, corroding, reactions.
   c. Engine objects collide with the cell world; explosions and digging edit it.
   d. Pixel rigid bodies: cells grouped into bodies (outline, convex parts) that simulate in
      crystalline, break apart, and are written back into the world each frame.
   e. Optional GPU compute path for large worlds.

6. **Effects authoring: particles and shader effects, built for the mega shader**
   **Done 2026-11-21** (uncommitted):
   - Engine: `wgpu_canvas::particles`. One instance per emitter; the vertex shader places
     every particle from the emitter's age, slot and an integer hash (`particles.wgsl`). There
     is no per-particle CPU work or upload. Neighbouring emitters with the same slot count and
     image share one draw. Shapes drawn from formulas (dot, disc, square, ring, star) all use the
     white pixel, so they batch across effects. Particles can be additive or blended, stretch
     along their velocity, take drag and gravity in closed form, and trail behind a moving
     emitter (`follow`). Eight presets are included. GPU readback tests cover the behaviour
     (`tests/particles_draw.rs`). Mega effects now run on real frame time.
   - quartz: `Canvas::play_particles` and the `PlayParticles` / `StopParticles` /
     `RemoveParticles` actions. Effects age with game time, are resolved in the same pass as
     their object (no lag), draw at the object's depth, and stop when the object goes.
   - Data: `data/effects/<name>.json` (`ParticleEffectAsset`), objects' `particles`, and the
     actions. Codegen writes the emitter inline and loads images once per program; import
     reads generated code back. The live player plays the effects, restarts an object whose
     effect file changes, and now attaches object shader effects too (`EffectBridge`, pinned
     field by field to codegen).
   - Editor: an Effects tab with the effect list, presets, a full form, a live preview drawn
     by the engine's own renderer (backdrops, zoom, pause), and the cost check. The object
     inspector has a Particle Effects section and plays its shader effect on request.
   - Enforcement: `ParticleEffectAsset::check`. Blocks what the pass cannot draw (no lifetime,
     no particles, an image shape without an image, a bad name): the editor marks it ✖ and the
     game does not play it. Warns about fill over one screen a frame and about images breaking
     batching. Reports particles alive at once and fill. The `qf_particle_effect` MCP tool
     refuses to save a blocked effect and prices every write.
   - Closed the same day (2026-11-21), the four items left open by the first slice:
     - Crystalline's simulated particles draw in the mega pass: one drawable per render layer,
       mega sprites grouped by shape mask, so each shape is one draw and the layer keeps its
       place in the render order (`quartz/src/canvas/particle_layer.rs`).
     - Object shader effects are priced per kind (`quartz_project/src/effect_cost.rs`). The
       weights are measured: `wgpu_canvas/tests/effect_kind_cost.rs` times each kind as the
       slope between 12 and 36 full-screen layers, so one-off frame costs cancel. Kinds cost
       1.5-3.2x a plain sprite per pixel. Animated looks add up (FIRE +3.9, ELECTRICITY +2.7,
       DISSOLVE +2.4, the rest +0.2-1.9). The table (`effect_costs.json`) records how it was
       measured: Apple M4 host. **A phone run should replace it**:
       `EFFECT_COST_TABLE=<file> cargo test --release --test effect_kind_cost` on the device.
       The check warns above one plain screen of cost. The inspector shows it under the
       effect, and `qf_particle_effect` prices every object shader effect in `list` and any
       spec in `shader_check`.
     - The scene view draws effects with the engine: every shader effect and particle effect in
       view, in one headless render laid over the editor (`app/effects/scene_overlay.rs`,
       "Show effects" in the view bar). Selection handles stay on top. The overlay lies over
       all objects, whereas the game draws an effect at its object's depth.
     - Particle looks take up to two keys over life (`LifeKey`: at, colour, size, either
       optional). The CPU resolves them into a fixed four-key ramp
       (`ParticleLook::ramp`), so the shader evaluates three straight segments with no key
       count. The instance grew from 11 to 14 vec4 (224 B), within the 16 vertex inputs
       every device takes. Editor: "Over life" section; data `look.keys`; codegen and MCP
       carry it; three keys are blocked.

7. **Check the games on the changed engine** (next, asked 2026-11-21). The synful_quartz
   changes above reach quartz_path_forge and hollow_road (the root `path_forge/` is not on
   this machine). Steps:
   - Run their tests, including shot timing and boss timing.
   - Compare desktop captures from before and after the change.
   - Run on the phone `ZA223HJX4F`, after asking the user.
   Main risk: mega effects now run on real frame time, so telegraphs play at true speed on a
   phone drawing under 60 fps (before, they slowed down with the frame rate).

8. **Project shader effects** (decided 2026-11-21). The 20 built-in kinds stay as the default
   kit, but the engine stops growing: new effects live in each game's project and reach the
   renderer at run time. Agents write WGSL against the contract. People design with a layer
   stack (patterns, modifiers, blend modes, drivers), which compiles to the same WGSL.
   - Done (2026-11-21, uncommitted):
     - Engine contract (see ENGINE_CHANGES); tests `wgpu_canvas/tests/project_shaders.rs`,
       `quartz/tests/project_effects.rs`.
     - Project files `data/shaders/<name>.json` (params, additive, authoring, measured cost)
       plus `<name>.wgsl`: hand-written, or the layer stack's generated WGSL so a generated
       game embeds the file whatever its authoring (`quartz_project/src/shader_asset.rs`,
       `store/shaders.rs`).
     - Layer stack model and its WGSL compiler (`shader_layers.rs`, `shader_compile.rs`):
       11 patterns, 9 warps, 6 shaping steps, 4 colour sources, 6 blend modes. Any number is
       fixed or driven by time, a wave, a parameter or a layer below. Every part compiles in
       the engine (`shader_engine.rs` test).
     - Objects choose a project shader (`ObjectEffectSpec::project`); the cost check prices it
       by its measured weight and blocks a missing shader.
     - Live player: one source per shader while its text is unchanged; an edit rebuilds the
       pipeline (`quartz_project_runtime/src/shaders.rs`, `tests/live_reload.rs`).
     - Codegen embeds the file with `include_str!`; import reads it back.
     - MCP `qf_shader` (list/get/set/delete/check/parts/contract): a write that does not
       build is refused with its line.
     - Forge: Effects tab Particles / Shaders switch (`app/effects/shaders/`). Layer cards
       top first, each with an engine thumbnail of the stack up to it; parameters; WGSL
       editor with the error line and the contract; convert layers to WGSL; live preview
       with tint, size and parameter sliders; the inspector's effect menu lists project
       shaders and plays them. Cost is measured on this GPU automatically 1.5 s after edits
       pause (slope between 12 and 36 full-screen layers at 1356x610, against a plain
       sprite), and stored with the source hash it holds for. The scene view draws an
       object's project shader too.
   - Next:
     - Measure on the phone: a measured weight says which GPU it came from; the phone's
       ratios should replace host ones before the check is trusted for a phone game.

## Still to do from the data-first slice

- Live mapping for object shader effects, backgrounds, lighting, post effects, pools, plugins.
- A shipped game that bundles the data and the player (phones), beside the generated-code build.
