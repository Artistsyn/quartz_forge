# Quartz Forge MCP Tool Catalog

This file documents the dedicated `quartz_forge_mcp` server surface and the intent behind each tool name.

## Tool Names

- `qf_api_lookup` - search local Quartz source and `quartz/api.txt` for exact native API symbols, signatures, and nearby usage.
- `qf_api_verify_snippet` - check a proposed snippet against local Quartz source before approving custom code.
- `qf_forge_check_parity` - compare quartz_forge domain coverage against Quartz `Action` and `Condition` variants.
- `qf_spawn_audit` - inspect first-class spawn-only workflow coverage, overlay readiness, and helper routing.
- `qf_project_lint_layout` - verify the workspace layout matches the expected quartz_forge module and binary boundaries.

## Routing Intent

- Prefer native Quartz forms before custom code.
- Prefer source-first answers over memory or cached summaries.
- Use parity checks to decide whether a UI/editor surface is genuinely represented or still missing.
- Keep tool names short, explicit, and policy-bearing so model routing is unambiguous.

## Current Server Commands

- `quartz_forge_mcp --stdio`
- `quartz_forge_mcp --health`
- `quartz_forge_mcp --lock-status`
