# Quartz Forge

Quartz Forge is a Quartz-native authoring environment inside the FlowMake workspace. Its job is to help humans and AI agents build real Quartz 2D game projects through source-backed scene editing, Quartz-native code generation, local API verification, and MCP-assisted workflows.

## What Quartz Forge Is For

Quartz Forge is meant to keep editor intent and runtime code close together.

| Goal | What it means |
| --- | --- |
| Quartz-native authoring | Generated output stays aligned with `quartz::prelude::*`, Quartz `Action` and `Condition` semantics, and local engine reality. |
| AI-assisted project building | Copilot and other agents should be able to use quartz_forge and its MCP tools to generate, extend, and maintain new Quartz game projects without inventing off-model engine syntax. |
| Editor-to-runtime continuity | Scene layout, object settings, actions, events, custom code blocks, and generated files all point toward shippable Rust output. |
| Workspace-aware validation | Quartz Forge validates against local Quartz source, local api.txt, and quartz_forge's own coverage so agents can route work through real supported surfaces. |

## Core Capabilities

### Visual Scene Authoring

- Interactive scene canvas with drag, resize, rotate, pan, zoom, grid snap, arrow-key nudging, and layer-ordered rendering.
- Object authoring for rectangles, circles, spawn-only templates, background objects, static images, and animated sprites.
- Camera-space pinning, pivot visualization, background-cell overlays, camera frame overlays, and spawn ghost overlays.
- Rotated asset previews now follow the object's rotation parameter in the editor, matching expected Quartz runtime behavior.

### Logic, Events, and Code Generation

- Update-script authoring that exports Quartz-native `canvas.on_update(...)` logic.
- Event builder aligned with Quartz event shapes such as key, mouse, collision, tick, and custom event flows.
- Action editing for movement, camera effects, plugin calls, grouped actions, conditional actions, variable ops, spawn actions, and text updates.
- Generated Quartz syntax preview plus one-click script write to generated Rust files.

### Multi-File Project Support

- Scene parts can target external files instead of a single monolithic scene source.
- Quartz Forge already auto-emits `#[path = "..."] mod ...;` and `use module::*;` lines for generated scene composition when objects, events, logic trees, or custom code blocks live in separate target files.
- This keeps generated Rust syntactically valid without asking users or agents to hand-wire basic module imports for supported quartz_forge output paths.

### MCP and Agent Workflows

- Dedicated `quartz_forge_mcp` server for local API lookup, snippet verification, parity checks, spawn audits, text guidance, and layout verification.
- Intended to let Copilot or other agents interface with quartz_forge itself when creating or extending Quartz projects.
- The MCP layer is meant to keep agent output Quartz-native, source-backed, and workspace-aware.

## MCP Purpose

Quartz Forge MCP is not only a syntax search endpoint.

Its purpose is to help agents:

- look up real Quartz API forms before generating code,
- verify that proposed snippets match local Quartz and quartz_forge support,
- understand text, spawn, and layout rules that quartz_forge already knows,
- generate new project code using Quartz-native syntax instead of ad-hoc wrappers.

### Current MCP tools

| Tool | Purpose |
| --- | --- |
| `qf_api_lookup` | Search local Quartz API/source for exact native symbols and signatures. |
| `qf_api_verify_snippet` | Validate snippets and warn about API drift or risky text patterns. |
| `qf_text_knowledge` | Return Quartz Forge's preferred text construction and font-caching guidance. |
| `qf_forge_check_parity` | Compare Quartz Forge support against Quartz action and condition enums. |
| `qf_spawn_audit` | Inspect spawn-only workflow coverage and helper routing. |
| `qf_project_lint_layout` | Report workspace layout expectations and generated multi-file module/use wiring. |

## Workspace Expectations

Quartz Forge currently expects to run inside the FlowMake workspace root.

Minimum expected layout:

```text
FlowMake/
  quartz/
  quartz_forge/
  quartz_ai_api_cache/
  .vscode/
```

Important assumptions:

- `quartz/api.txt` must exist.
- `quartz/src/types/action.rs` and `quartz/src/types/condition.rs` must exist.
- `quartz_forge/src/core/quartz_domain.rs` is used as the editor-side truth for parity checks.
- Quartz Forge MCP auto-discovers the FlowMake root by walking upward from its current directory.

## Quick Start

### Run the editor

From the FlowMake root:

```powershell
cargo run --manifest-path quartz_forge/Cargo.toml --bin quartz_forge
```

### Open or create a project

- Launch Quartz Forge.
- Create a new project or open an existing Quartz Forge project.
- Use the scene canvas, object menu, event builder, and custom code windows to start authoring.

### Generate and inspect output

- Use the generated Quartz preview to inspect the current export shape.
- Write generated files into your project scripts when ready.
- Use the generated-file browser to inspect or track manual overrides.

## MCP Setup

Quartz Forge includes a dedicated MCP binary: `quartz_forge_mcp`.

Health check:

```powershell
cargo run --manifest-path quartz_forge/Cargo.toml --bin quartz_forge_mcp -- --health
```

Run over stdio:

```powershell
cargo run --manifest-path quartz_forge/Cargo.toml --bin quartz_forge_mcp -- --stdio
```

The FlowMake workspace `.vscode/mcp.json` can point VS Code at this server so chat agents can use the quartz_forge MCP surface directly.

## Text Guidance

Quartz Forge now follows the same text pattern found in `ball_swing_game`.

Preferred pattern:

- Build text directly with `Text::new(vec![Span::new(...)], ...)`.
- Cache fonts with `OnceLock<Font>` plus `Font::from_bytes(include_bytes!(...))`.
- Use `Some(font_size * 1.25)` for line height unless you have a specific reason not to.
- Build the `Text` value before calling `Action::SetText` or mutating an object's drawable.

Avoid:

- Treating `canvas.make_text(...)` as the main generated pattern when direct `Text::new` plus `Span::new` is available.
- Creating text inside a `get_game_object_mut(...)` borrow.
- Re-parsing the same font bytes every frame.

## Windowing Notes

- The existing collapsible tool windows now dock into a fixed tray when collapsed so they stay easy to find without covering the workspace.
- Restoring a docked window reopens it through the same egui window id, which preserves its prior position rather than snapping it to a fresh default.

## Architecture Notes

Quartz Forge is intentionally split into focused layers:

- `src/app/` for egui orchestration and editor windows.
- `src/core/` for project and Quartz-domain models.
- `src/services/` for code generation, persistence, and preview/hot-reload support.
- `src/mcp.rs` and `src/bin/quartz_forge_mcp.rs` for the MCP server surface.

That separation keeps UI concerns out of the source-of-truth model and makes it easier for both humans and agents to validate generated Rust against local Quartz APIs.

## Runbook

Useful commands from the FlowMake root:

```powershell
cargo check --manifest-path quartz_forge/Cargo.toml
cargo run --manifest-path quartz_forge/Cargo.toml --bin quartz_forge
cargo run --manifest-path quartz_forge/Cargo.toml --bin quartz_forge_mcp -- --health
```