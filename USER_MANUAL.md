# Quartz Forge User Manual

## Quick Links

- Project overview: [README.md](README.md)
- Operator checklist: [OPERATOR_CHECKLIST.md](OPERATOR_CHECKLIST.md)
- API coverage checklist: [QUARTZ_API_COVERAGE_CHECKLIST.md](QUARTZ_API_COVERAGE_CHECKLIST.md)
- MCP tool catalog: [QUARTZ_FORGE_MCP_TOOL_CATALOG.md](QUARTZ_FORGE_MCP_TOOL_CATALOG.md)
- MCP implementation plan: [QUARTZ_FORGE_MCP_SERVER_IMPLEMENTATION_PLAN.md](QUARTZ_FORGE_MCP_SERVER_IMPLEMENTATION_PLAN.md)

## 1. What Quartz Forge Is

Quartz Forge is a Quartz-native authoring environment for building 2D game projects in the FlowMake workspace.

It gives you:

- visual scene/object authoring
- structured project state (manifest-based)
- generated Rust scene and component files
- semantic import/export workflows
- MCP tools for source-backed AI workflows and validation

## 2. Who This Manual Is For

This guide is for:

- game developers using Quartz Forge directly
- teams integrating AI assistants into project workflows
- users outside VS Code/Copilot who still want MCP-powered automation

## 3. Current Maturity and Expectations

Quartz Forge is in active hardening.

At this stage:

- import/export is functional and improving, but not perfect
- semantic import still has edge cases on certain source shapes
- API coverage is broad and growing, not final
- behavior-preserving organization changes in generated files are expected
- we are actively gathering feedback to reach release-readiness, but Quartz Forge should still be treated as pre-release tooling

Operational recommendation:

- treat each import/export as a validated step, not a blind trust step
- always run a build and quick runtime check after major cycles

## 4. Prerequisites

Workspace shape expected:

```text
FlowMake/
  quartz/
  quartz_forge/
  quartz_ai_api_cache/
```

Required source files used by MCP/parity checks:

- `quartz/api.txt`
- `quartz/src/types/action.rs`
- `quartz/src/types/condition.rs`

Tooling:

- Rust toolchain and Cargo
- Windows PowerShell examples are provided in this manual

## 5. Install and Run

Run editor from workspace root:

```powershell
cargo run --manifest-path quartz_forge/Cargo.toml --bin quartz_forge
```

Basic compile check:

```powershell
cargo check --manifest-path quartz_forge/Cargo.toml
```

## 6. MCP Setup (All Clients)

Quartz Forge MCP speaks JSON-RPC over stdio.

### 6.1 Build MCP server

Option A: dedicated MCP crate (commonly used in this workspace)

```powershell
Push-Location quartz_forge/mcp_server
cargo build
Pop-Location
```

Option B: workspace bin

```powershell
cargo build --manifest-path quartz_forge/Cargo.toml --bin quartz_forge_mcp
```

### 6.2 Run health checks

```powershell
cargo run --manifest-path quartz_forge/Cargo.toml --bin quartz_forge_mcp -- --health
cargo run --manifest-path quartz_forge/Cargo.toml --bin quartz_forge_mcp -- --lock-status
```

### 6.3 Run MCP server

```powershell
cargo run --manifest-path quartz_forge/Cargo.toml --bin quartz_forge_mcp -- --stdio
```

### 6.4 VS Code/Copilot configuration

Example `.vscode/mcp.json`:

```json
{
  "servers": {
    "quartz_forge_mcp": {
      "command": "c:/Users/ArtistRyzenWhite/RProjects/FlowMake/quartz_forge/mcp_server/target/debug/quartz_forge_mcp.exe",
      "args": ["--stdio"]
    }
  }
}
```

### 6.5 Non-VSCode/non-Copilot MCP configuration

Any MCP-compatible client can use Quartz Forge MCP with a stdio server entry.

Use:

```text
command: <path-to-quartz_forge_mcp binary>
args: ["--stdio"]
```

Required methods implemented:

- `initialize`
- `tools/list`
- `tools/call`
- `ping`

## 7. Core User Workflows

### 7.1 New project flow

1. Start Quartz Forge.
2. Create/open project.
3. Author scene, objects, events, logic.
4. Export generated files.
5. Build and run target game.
6. Re-import semantically if needed.

### 7.2 Existing project flow

1. Backup key scene/source files.
2. Import semantically.
3. Review generated diff and sync status.
4. Fix drift if needed.
5. Rebuild and validate behavior.

### 7.3 AI-assisted flow

1. Use `qf_codegen_api_guidance` first.
2. Use `qf_api_lookup` and `qf_api_verify_snippet` before large code changes.
3. Use `qf_project_state_dump` and `qf_project_apply_state` for state-level edits.
4. Use `qf_project_sync_status` after generation/import cycles.
5. Run the full operator cycle in [OPERATOR_CHECKLIST.md](OPERATOR_CHECKLIST.md).

## 8. High-Value MCP Tools

- `qf_api_lookup`: local source-backed lookup for native API symbols and signatures.
- `qf_api_verify_snippet`: catches drift and common generation mistakes early.
- `qf_project_roundtrip_contract`: explains roundtrip assumptions and current contract.
- `qf_project_state_dump`: returns editable manifest state and sync report.
- `qf_project_apply_state`: applies full manifest and optionally regenerates output.
- `qf_project_import_semantic`: imports supported files into manifest state.
- `qf_project_sync_status`: reports save/export drift.
- `qf_forge_check_parity`: compares forge support against Quartz action/condition surfaces.
- `qf_project_lint_layout`: validates project layout and module routing expectations.

## 9. MCP Readiness Audit (AI Coding Workflows)

Before large AI-driven project changes, verify this checklist:

1. MCP binary launches and responds to `--health`.
2. Client can call `tools/list` and sees the Quartz Forge tool surface.
3. `qf_codegen_api_guidance` is called first for game logic tasks.
4. `qf_api_lookup` and `qf_api_verify_snippet` are used for uncertain API shapes.
5. `qf_project_state_dump` and `qf_project_apply_state` are preferred for stateful project edits.
6. `qf_project_sync_status` is checked after import/export operations.
7. `qf_forge_check_parity` is used when introducing new Action/Condition-driven features.

If any item fails, treat AI generation output as low confidence until the MCP path is restored.

## 10. Known Cautions

Current caution areas:

- semantic import omissions in some complex code shapes
- generated ordering differences (often non-functional, but still diff-noisy)
- ongoing API parity expansion for action/condition/editor/codegen surfaces
- occasional file-generation omissions detected during real project cycles

Practical safety rules:

- never skip diff review on important scene files
- validate behavior with a quick runtime smoke test
- report omissions with exact file diff + runtime symptom
- use short cycles: export -> build -> import -> diff -> build

## 11. Troubleshooting

### MCP server not found

- verify binary path exists
- run `--health` and `--lock-status`
- confirm your MCP client is launching with `--stdio`

### Import/export drift

- run semantic import on a narrow file set first
- inspect sync status
- compare before/after diff in key scene files
- verify if differences are organizational vs behavioral

### Compile errors after generation

- inspect whether setup-runtime statements are emitted before/after object moves
- verify object local mutations happen before `canvas.add_game_object`
- run parity checks and snippet verification for suspicious sections
- verify no-collision objects remain non-colliding (`collision_layer(0)`, `collision_mask(0)`, non-platform)
- verify local-reference rewrite does not drop valid statements when object id contains a local-name substring

## 12. Reporting Bugs Effectively

When reporting a Quartz Forge issue, include:

- project root and file paths affected
- exact import/export steps
- compile/runtime errors
- minimal reproducible diff
- expected behavior vs actual behavior

This reduces turnaround time significantly for importer/codegen fixes.

Feedback priority (highest to lowest):

1. compile-breaking generation errors
2. behavior regressions after roundtrip
3. missing import/export coverage
4. usability and workflow friction

## 13. Recommended Team Policy

For teams using Quartz Forge in production-like workflows:

1. keep manual backups before first import on a branch
2. require diff + build validation for every import/export cycle
3. keep MCP health checks in CI/dev startup scripts
4. track known drift patterns in team notes
5. re-run parity checks after major Quartz engine updates

## 14. Final Notes

Quartz Forge is already powerful for Quartz-native workflows, but it is still in active stabilization. If you treat it as a validated generation/import system rather than a black box, it can be used safely and efficiently today.