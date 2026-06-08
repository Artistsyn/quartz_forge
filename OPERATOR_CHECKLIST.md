# Quartz Forge Operator Checklist

Use this runbook for import/export drift triage, AI-assisted generation, and roundtrip validation.

Status note:

- Quartz Forge is pre-release and under active stabilization.
- Treat this checklist as required for high-confidence workflows.

## 1. Preflight (Before Any Large Edit)

1. Confirm MCP server health:
   - `cargo run --manifest-path quartz_forge/Cargo.toml --bin quartz_forge_mcp -- --health`
2. Confirm MCP client can call `tools/list` and expose Quartz Forge tools.
3. Call `qf_codegen_api_guidance` before generating gameplay logic.
4. If API shape is uncertain, call `qf_api_lookup` and `qf_api_verify_snippet`.
5. Create a backup or branch checkpoint for key scene/source files.

## 2. Authoring Rules (Must Hold)

1. Prefer project-state workflows:
   - `qf_project_state_dump` -> edit state -> `qf_project_apply_state`
2. Use `qf_project_import_semantic` with fallback enabled when source shapes are uncertain.
3. Keep setup generation ordering safe:
   - build object locals
   - emit setup runtime/game vars while locals are alive
   - move objects into canvas last
4. Preserve no-collision semantics for non-colliders:
   - `collision_layer(0)` and `collision_mask(0)` and non-platform behavior
5. Use token-aware identifier rewrite logic for builder-local replacement; never substring-match locals.

## 3. Roundtrip Validation Loop (Mandatory)

1. Export generated files.
2. Build and run target game.
3. Semantic import changed files.
4. Run sync status and inspect drift:
   - `qf_project_sync_status`
5. Diff key scene/component files.
6. Rebuild and runtime-smoke test.

Stop and fix before continuing if any step fails.

## 4. Drift Triage Decision Tree

1. Compile breaks after export/import:
   - first inspect setup ordering and moved-value mistakes
   - then inspect local-reference rewrite edge cases
2. Runtime behavior changed but compiles:
   - classify as organizational diff vs behavioral regression
   - verify movement/collision semantics first
3. Missing import/export coverage:
   - preserve affected code via ManualFileOverride fallback
   - report gap with minimal reproducible example

## 5. Acceptance Gates

A cycle is complete only when all are true:

1. Build passes.
2. Runtime smoke passes for affected gameplay.
3. Sync status is understood and accepted.
4. Drift diff has no unreviewed high-risk behavior changes.
5. API parity assumptions are validated if new Action/Condition flows were introduced (`qf_forge_check_parity`).

## 6. Incident Report Template

When filing a drift/import-export issue, include:

1. project root and exact file paths
2. precise command sequence executed
3. expected vs actual behavior
4. minimal diff snippet
5. compile/runtime errors
6. MCP tool outputs used (`qf_project_sync_status`, `qf_api_verify_snippet`, parity checks)

## 7. Fast Command Block

```powershell
cargo run --manifest-path quartz_forge/Cargo.toml --bin quartz_forge_mcp -- --health
cargo check --manifest-path quartz_forge/Cargo.toml
```
