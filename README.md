# Quartz Forge

Quartz Forge is a Quartz-native game authoring environment for the FlowMake workspace. It combines scene editing, structured manifest state, Rust code generation, and an MCP server so both humans and AI agents can build Quartz projects with source-backed constraints.

## Quick Links

- User manual: [USER_MANUAL.md](USER_MANUAL.md)
- Operator checklist: [OPERATOR_CHECKLIST.md](OPERATOR_CHECKLIST.md)
- API coverage checklist: [QUARTZ_API_COVERAGE_CHECKLIST.md](QUARTZ_API_COVERAGE_CHECKLIST.md)
- MCP tool catalog: [QUARTZ_FORGE_MCP_TOOL_CATALOG.md](QUARTZ_FORGE_MCP_TOOL_CATALOG.md)
- MCP implementation plan: [QUARTZ_FORGE_MCP_SERVER_IMPLEMENTATION_PLAN.md](QUARTZ_FORGE_MCP_SERVER_IMPLEMENTATION_PLAN.md)

## Current Stage (Read First)

Quartz Forge is actively evolving. It is usable and productive, but still under heavy import/export hardening and API parity expansion.

What this means in practice:

- You should assume import and file-generation edge cases still exist.
- Roundtrip behavior is improving quickly, but not all source shapes are semantically imported with full fidelity yet.
- API coverage is broad, but not complete for every Action/Condition/editor workflow.
- Manual verification after major import/export cycles is still recommended.
- We are explicitly collecting user feedback to drive stabilization toward release-readiness, but we are not release-ready yet.

## What Quartz Forge Does

- Visual authoring for scenes and objects (position, size, rotation, layers, collision and advanced params).
- Logic/event authoring that maps to Quartz-native runtime code.
- Structured project state with generated Rust output.
- Multi-file generation support with module wiring for supported component targets.
- MCP tools for API lookup, snippet verification, parity checks, project state operations, and roundtrip diagnostics.

## Workspace Requirements

Quartz Forge expects a FlowMake-style workspace layout:

```text
FlowMake/
  quartz/
  quartz_forge/
  quartz_ai_api_cache/
```

Required files used by MCP and parity checks:

- `quartz/api.txt`
- `quartz/src/types/action.rs`
- `quartz/src/types/condition.rs`

## Run Quartz Forge

From workspace root:

```powershell
cargo run --manifest-path quartz_forge/Cargo.toml --bin quartz_forge
```

Health check:

```powershell
cargo check --manifest-path quartz_forge/Cargo.toml
```

## MCP Setup

Quartz Forge MCP supports standard JSON-RPC over stdio. This works with VS Code/Copilot and with non-VSCode/non-Copilot MCP clients.

### 1) Build MCP server binary

Option A (recommended in this repo):

```powershell
Push-Location quartz_forge/mcp_server
cargo build
Pop-Location
```

Option B (workspace crate bin):

```powershell
cargo build --manifest-path quartz_forge/Cargo.toml --bin quartz_forge_mcp
```

### 2) Verify server health

```powershell
cargo run --manifest-path quartz_forge/Cargo.toml --bin quartz_forge_mcp -- --health
```

### 3) Run MCP over stdio

```powershell
cargo run --manifest-path quartz_forge/Cargo.toml --bin quartz_forge_mcp -- --stdio
```

### VS Code/Copilot wiring

Example local config in `.vscode/mcp.json`:

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

### Non-VSCode / non-Copilot MCP clients

Use the same stdio command and register it in your client's MCP configuration (as a stdio server command + args).

Command template:

```text
command: <path-to-quartz_forge_mcp>
args: ["--stdio"]
```

Minimal JSON-RPC methods expected by this server:

- `initialize`
- `tools/list`
- `tools/call`
- `ping`

Operational flags:

- `--stdio` start MCP server loop
- `--health` print JSON health summary
- `--lock-status` print lock/heartbeat status JSON

## Major MCP Tools (High Value)

- `qf_api_lookup`: source-backed API lookup (`quartz/api.txt`, action/condition source, forge domain source)
- `qf_api_verify_snippet`: validate snippet shape and detect common drift/anti-patterns
- `qf_project_state_dump`: load project and return structured manifest + sync report
- `qf_project_apply_state`: write manifest and optionally regenerate files/snapshot
- `qf_project_import_semantic`: import supported files into manifest; can fallback to ManualFileOverride
- `qf_project_sync_status`: report save/export drift status
- `qf_forge_check_parity`: compare editor/codegen support with Quartz action/condition surfaces
- `qf_project_lint_layout`: validate project layout and generated routing assumptions

## MCP Adequacy Checklist For AI Coding

Use this checklist to ensure an AI workflow is grounded in current Quartz Forge constraints:

1. Start with `qf_codegen_api_guidance` before generating gameplay code.
2. Validate uncertain API usage with `qf_api_lookup` and `qf_api_verify_snippet`.
3. For project edits, prefer `qf_project_state_dump` plus `qf_project_apply_state` over ad-hoc Rust rewrites.
4. After generation/import, run `qf_project_sync_status` and review drift before further edits.
5. Use `qf_forge_check_parity` when adding new Action/Condition-driven workflows.
6. Keep import fallback enabled (`qf_project_import_semantic` with `fallback_manual_overrides=true`) until full semantic coverage is confirmed for your file shapes.

## Cautions and Known Risk Areas

Use these as operational guardrails while the project is still under active stabilization:

- Semantic import coverage is still evolving for complex source shapes.
- Generated file organization can differ from hand-authored ordering even when behavior is preserved.
- Roundtrip cycles can expose omissions or reorderings; validate diffs after import/export.
- Prefer incremental changes and short validation loops over large one-shot project rewrites.
- Keep backups of hand-authored files before first semantic import on older projects.

## Recommended Validation Loop

1. Export generated files.
2. Build and run game (`cargo run` in target project).
3. Re-import semantically.
4. Diff key scene files.
5. Rebuild and verify behavior.
6. Report drift/omissions with file diff and runtime symptom.

This loop is mandatory for high-confidence AI-assisted workflows at the current project stage.

For operator-friendly execution, use [OPERATOR_CHECKLIST.md](OPERATOR_CHECKLIST.md).

## Architecture Overview

- `src/app/`: egui editor windows and interaction orchestration
- `src/core/`: project and Quartz-domain models
- `src/services/`: codegen, import, persistence, sync
- `src/mcp.rs`: MCP server entry and tool routing
- `src/bin/quartz_forge_mcp.rs`: MCP bin launcher

## User Manual

For full setup, workflows, troubleshooting, and AI-agent guidance, see [USER_MANUAL.md](USER_MANUAL.md).