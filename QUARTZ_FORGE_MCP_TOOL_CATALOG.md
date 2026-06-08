# Quartz Forge MCP Tool Catalog

This file documents the dedicated `quartz_forge_mcp` server surface and the intent behind each tool name.

Quartz Forge MCP is not just a passive lookup layer. Its purpose is to let Copilot or other AI agents interface with quartz_forge itself so they can create, extend, and maintain Quartz 2D game projects using Quartz-native code syntax with local source-backed validation.

## Primary Purpose

- Help agents generate and evolve Quartz-native projects through quartz_forge-aware workflows.
- Keep generated code anchored to `quartz::prelude::*` semantics instead of ad-hoc engine guesses.
- Validate snippets, parity assumptions, and workspace structure against the local repo before code is emitted.
- Surface high-value editor truths such as text patterns, spawn coverage, and multi-file module wiring.

## Tool Names

- `qf_api_lookup` - search local Quartz source and `quartz/api.txt` for exact native API symbols, signatures, and nearby usage.
- `qf_api_verify_snippet` - check a proposed snippet against local Quartz source before approving custom code, including warnings for risky text patterns.
- `qf_text_knowledge` - return the current Quartz Forge text-authoring guidance, including direct `Text::new` + `Span::new`, OnceLock font caching, and `make_text` caveats.
- `qf_project_roundtrip_contract` - return the manifest/runtime scaffold contract an agent must preserve so a generated project stays editable in quartz_forge as well as runnable under Quartz.
- `qf_project_create` - create a new quartz_forge-native project root with default manifest/scaffold files and optional initial generated exports.
- `qf_project_state_dump` - return the structured quartz_forge manifest plus sync status for a target project root so an agent can edit project data directly.
- `qf_project_apply_state` - apply a full manifest state back to a project root and optionally rewrite generated files plus sync snapshot.
- `qf_project_import_semantic` - semantically import supported project files back into manifest state and fall back to `ManualFileOverride` for files that exceed the current importer contract.
- `qf_project_import_manual_overrides` - import selected Rust files into manifest metadata as `ManualFileOverride` blocks so manual work is preserved.
- `qf_project_sync_status` - inspect a target quartz_forge project root for manifest/export drift so an agent can decide whether to rewrite files, restore the last exported project state, or stop before causing work loss.
- `qf_forge_check_parity` - compare quartz_forge domain coverage against Quartz `Action` and `Condition` variants.
- `qf_spawn_audit` - inspect first-class spawn-only workflow coverage, overlay readiness, and helper routing.
- `qf_project_lint_layout` - verify the workspace layout matches the expected quartz_forge module and binary boundaries, including external-file `#[path] mod` and `use module::*` generation.

## Routing Intent

- Prefer native Quartz forms before custom code.
- Prefer source-first answers over memory or cached summaries.
- Use parity checks to decide whether a UI/editor surface is genuinely represented or still missing.
- Use quartz_forge MCP as an authoring assistant for project generation, not just as a syntax search endpoint.
- Keep tool names short, explicit, and policy-bearing so model routing is unambiguous.

## Adequacy For AI Project Coding

Quartz Forge MCP is currently adequate for guided AI-assisted project coding when used with a strict workflow:

1. Call `qf_codegen_api_guidance` before gameplay/event logic generation.
2. Validate non-trivial snippets with `qf_api_verify_snippet`.
3. Use `qf_project_state_dump` and `qf_project_apply_state` for project-state changes.
4. Use `qf_project_import_semantic` with fallback enabled during active importer hardening.
5. Run `qf_project_sync_status` after generation/import cycles and stop on unexpected drift.
6. Use `qf_forge_check_parity` before assuming unsupported Action/Condition coverage is available.

### Nuances discovered during import/export hardening

- setup_scene generation must keep local object mutations before object moves into canvas.
- setup_runtime preservation must rewrite builder-local references token-aware, not by substring.
- no-collision object semantics must preserve zero layer/mask to avoid accidental colliders.
- Organizational diffs can be behavior-preserving; compile/runtime validation is still required.

## Current Server Commands

- `quartz_forge_mcp --stdio`
- `quartz_forge_mcp --health`
- `quartz_forge_mcp --lock-status`
