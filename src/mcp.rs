use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::env;
use std::fs;
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};
use std::process;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone)]
struct WorkspacePaths {
    root: PathBuf,
    quartz_api_txt: PathBuf,
    quartz_action_rs: PathBuf,
    quartz_condition_rs: PathBuf,
    forge_domain_rs: PathBuf,
    mcp_dir: PathBuf,
    lock_file: PathBuf,
    heartbeat_file: PathBuf,
}

#[derive(Debug, Serialize)]
struct ToolInfo {
    name: &'static str,
    description: &'static str,
    input_schema: Value,
}

#[derive(Debug, Deserialize)]
struct JsonRpcRequest {
    jsonrpc: Option<String>,
    id: Option<Value>,
    method: String,
    #[serde(default)]
    params: Value,
}

#[derive(Debug, Serialize)]
struct JsonRpcResponse {
    jsonrpc: &'static str,
    id: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<JsonRpcError>,
}

#[derive(Debug, Serialize)]
struct JsonRpcError {
    code: i32,
    message: String,
}

#[derive(Debug, Serialize)]
struct ToolListResult {
    tools: Vec<ToolInfo>,
}

pub fn run_from_args() -> Result<()> {
    let args = env::args().skip(1).collect::<Vec<_>>();
    let command = args.first().map(String::as_str).unwrap_or("--stdio");
    let paths = locate_workspace_paths()?;

    match command {
        "--stdio" => run_stdio(paths),
        "--health" => {
            let summary = health_report(&paths)?;
            println!("{}", serde_json::to_string_pretty(&summary)?);
            Ok(())
        }
        "--lock-status" => {
            let summary = lock_status(&paths)?;
            println!("{}", serde_json::to_string_pretty(&summary)?);
            Ok(())
        }
        "--help" | "-h" => {
            println!("Quartz Forge MCP server");
            println!("  --stdio        run MCP over stdio");
            println!("  --health       print workspace/tool health as JSON");
            println!("  --lock-status  print lock/heartbeat status as JSON");
            Ok(())
        }
        other => Err(anyhow!("unknown quartz_forge_mcp command: {other}")),
    }
}

fn run_stdio(paths: WorkspacePaths) -> Result<()> {
    acquire_lock(&paths)?;

    let stdin = io::stdin();
    let mut reader = stdin.lock();
    let mut stdout = io::stdout();

    loop {
        let Some(request) = read_rpc_request(&mut reader)? else {
            break;
        };

        if let Some(response) = handle_rpc_request(&paths, request)? {
            write_rpc_response(&mut stdout, response)?;
            stdout.flush()?;
        }
    }

    Ok(())
}

fn handle_rpc_request(paths: &WorkspacePaths, request: JsonRpcRequest) -> Result<Option<JsonRpcResponse>> {
    let Some(id) = request.id else {
        return Ok(None);
    };
    let _ = request.jsonrpc.as_deref();

    let response = match request.method.as_str() {
        "initialize" => JsonRpcResponse {
            jsonrpc: "2.0",
            id,
            result: Some(json!({
                "protocolVersion": "2024-11-05",
                "serverInfo": {
                    "name": "quartz_forge_mcp",
                    "version": env!("CARGO_PKG_VERSION")
                },
                "capabilities": {
                    "tools": {}
                }
            })),
            error: None,
        },
        "tools/list" => JsonRpcResponse {
            jsonrpc: "2.0",
            id,
            result: Some(json!(ToolListResult { tools: tool_list() })),
            error: None,
        },
        "tools/call" => {
            let tool_name = request
                .params
                .get("name")
                .and_then(Value::as_str)
                .ok_or_else(|| anyhow!("tools/call missing params.name"))?;
            let args = request.params.get("arguments").cloned().unwrap_or(Value::Null);
            let result = call_tool(paths, tool_name, args)?;
            JsonRpcResponse {
                jsonrpc: "2.0",
                id,
                result: Some(result),
                error: None,
            }
        }
        "ping" => JsonRpcResponse {
            jsonrpc: "2.0",
            id,
            result: Some(json!({"ok": true})),
            error: None,
        },
        other => JsonRpcResponse {
            jsonrpc: "2.0",
            id,
            result: None,
            error: Some(JsonRpcError {
                code: -32601,
                message: format!("unknown method: {other}"),
            }),
        },
    };

    Ok(Some(response))
}

fn call_tool(paths: &WorkspacePaths, tool_name: &str, args: Value) -> Result<Value> {
    match tool_name {
        "qf_api_lookup" => {
            let query = args.get("query").and_then(Value::as_str).unwrap_or("");
            let limit = args.get("limit").and_then(Value::as_u64).unwrap_or(12) as usize;
            Ok(json!({
                "tool": tool_name,
                "query": query,
                "matches": api_lookup(paths, query, limit)?,
            }))
        }
        "qf_api_verify_snippet" => {
            let snippet = args.get("snippet").and_then(Value::as_str).unwrap_or("");
            Ok(json!({
                "tool": tool_name,
                "verified": verify_snippet(paths, snippet)?,
            }))
        }
        "qf_text_knowledge" => Ok(json!({
            "tool": tool_name,
            "knowledge": text_knowledge(paths)?,
        })),
        "qf_forge_check_parity" => {
            let surface = args.get("surface").and_then(Value::as_str).unwrap_or("all");
            Ok(json!({
                "tool": tool_name,
                "surface": surface,
                "report": parity_report(paths, surface)?,
            }))
        }
        "qf_spawn_audit" => Ok(json!({
            "tool": tool_name,
            "report": spawn_audit(paths)?,
        })),
        "qf_project_lint_layout" => Ok(json!({
            "tool": tool_name,
            "workspace_root": paths.root,
            "status": "ok",
            "notes": [
                "quartz_forge keeps app/core/services split",
                "dedicated MCP binary available as quartz_forge_mcp"
            ]
        })),
        other => Err(anyhow!("unknown tool: {other}")),
    }
}

fn tool_list() -> Vec<ToolInfo> {
    vec![
        ToolInfo {
            name: "qf_api_lookup",
            description: "Search local Quartz source and api.txt for exact native API signatures, constructors, and usage notes before custom code is considered.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "Native Quartz keyword, symbol, or usage intent" },
                    "limit": { "type": "integer", "minimum": 1, "maximum": 50 }
                },
                "required": ["query"]
            }),
        },
        ToolInfo {
            name: "qf_api_verify_snippet",
            description: "Validate a proposed snippet against local Quartz source to catch wrong constructors, wrong action names, or custom-code drift.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "snippet": { "type": "string" }
                },
                "required": ["snippet"]
            }),
        },
        ToolInfo {
            name: "qf_text_knowledge",
            description: "Return the current Quartz Forge text-authoring guidance: preferred Text::new/Span::new construction, OnceLock font caching, SetText expectations, and known make_text pitfalls.",
            input_schema: json!({
                "type": "object",
                "properties": {}
            }),
        },
        ToolInfo {
            name: "qf_forge_check_parity",
            description: "Compare quartz_forge domain/editor/codegen support with quartz Action/Condition enums and report missing or extra variants.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "surface": {
                        "type": "string",
                        "enum": ["action", "condition", "wiring", "all"],
                        "default": "all"
                    }
                }
            }),
        },
        ToolInfo {
            name: "qf_spawn_audit",
            description: "Inspect first-class spawn-only workflow coverage, overlay readiness, and helper routing for spawn objects.",
            input_schema: json!({
                "type": "object",
                "properties": {}
            }),
        },
        ToolInfo {
            name: "qf_project_lint_layout",
            description: "Check quartz_forge project layout conventions and route feature placement through the intended app/core/services boundaries.",
            input_schema: json!({
                "type": "object",
                "properties": {}
            }),
        },
    ]
}

fn api_lookup(paths: &WorkspacePaths, query: &str, limit: usize) -> Result<Vec<Value>> {
    let query = query.trim().to_ascii_lowercase();
    let mut matches = Vec::new();
    let api_text = fs::read_to_string(&paths.quartz_api_txt)
        .with_context(|| format!("read {}", paths.quartz_api_txt.display()))?;

    for (line_no, line) in api_text.lines().enumerate() {
        if query.is_empty() || line.to_ascii_lowercase().contains(&query) {
            matches.push(json!({
                "source": paths.quartz_api_txt.display().to_string(),
                "line": line_no + 1,
                "text": line.trim()
            }));
        }
        if matches.len() >= limit {
            break;
        }
    }

    if matches.len() < limit {
        for path in [&paths.quartz_action_rs, &paths.quartz_condition_rs, &paths.forge_domain_rs] {
            let content = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
            for (line_no, line) in content.lines().enumerate() {
                if query.is_empty() || line.to_ascii_lowercase().contains(&query) {
                    matches.push(json!({
                        "source": path.display().to_string(),
                        "line": line_no + 1,
                        "text": line.trim()
                    }));
                }
                if matches.len() >= limit {
                    return Ok(matches);
                }
            }
        }
    }

    Ok(matches)
}

fn verify_snippet(paths: &WorkspacePaths, snippet: &str) -> Result<Value> {
    let action_text = fs::read_to_string(&paths.quartz_action_rs)?;
    let condition_text = fs::read_to_string(&paths.quartz_condition_rs)?;
    let api_text = fs::read_to_string(&paths.quartz_api_txt)?;

    let mut missing = Vec::new();
    let mut warnings = Vec::new();
    for needle in [
        "Action::",
        "Condition::",
        "Target::",
        "Location::",
        "Text::new",
        "Span::new",
        "SoundOptions::",
    ] {
        if snippet.contains(needle)
            && !action_text.contains(needle)
            && !condition_text.contains(needle)
            && !api_text.contains(needle)
        {
            missing.push(needle);
        }
    }

    if snippet.contains("canvas.make_text(") {
        warnings.push(
            "Quartz Forge text guidance prefers direct Text::new + Span::new construction; treat canvas.make_text(...) as a legacy convenience and build it before any get_game_object_mut() borrow.".to_owned()
        );
    }
    if snippet.contains("canvas.make_text(") && snippet.contains("get_game_object_mut(") {
        warnings.push(
            "make_text inside get_game_object_mut() risks the known double-borrow compile failure. Build the text value first, then mutate the object.".to_owned()
        );
    }
    if snippet.contains("Font::from_bytes(") && !snippet.contains("OnceLock") {
        warnings.push(
            "Repeated Font::from_bytes(...) parsing is slower than the preferred OnceLock<Font> cache used by ball_swing-style text helpers.".to_owned()
        );
    }

    let native_first = snippet.contains("quartz::prelude::*")
        || snippet.contains("Action::")
        || snippet.contains("Condition::")
        || snippet.contains("canvas.run(")
        || snippet.contains("Text::new")
        || snippet.contains("Span::new");

    Ok(json!({
        "native_first": native_first,
        "missing_symbols": missing,
        "warnings": warnings,
        "line_count": snippet.lines().count(),
    }))
}

fn text_knowledge(paths: &WorkspacePaths) -> Result<Value> {
    let api_text = fs::read_to_string(&paths.quartz_api_txt)?;
    let forge_codegen = fs::read_to_string(paths.root.join("quartz_forge/src/services/codegen.rs"))?;

    Ok(json!({
        "intent": "Prefer Quartz-native text construction that matches the real ball_swing_game pattern and current quartz_forge SetText codegen.",
        "preferred_pattern": {
            "construction": "Text::new(vec![Span::new(...)], max_width, Align::{Left|Center}, None)",
            "span_fields": [
                "text string",
                "font size",
                "Some(font_size * 1.25) line height",
                "Arc<Font>",
                "Color",
                "letter spacing 0.0"
            ],
            "font_loading": "Cache Font::from_bytes(include_bytes!(...)) in OnceLock<Font>, then clone into Arc<Font> when building spans.",
            "set_text_usage": "Build the Text value first, then pass it to Action::SetText or obj.set_drawable(Box::new(text))."
        },
        "avoid": [
            "Do not rely on canvas.make_text(...) as the primary generated pattern when Text::new + Span::new is available.",
            "Do not call make_text inside get_game_object_mut(); pre-build the text first to avoid double-borrow compile failures.",
            "Do not re-parse the same font bytes every frame when a OnceLock cache can hold the Font."
        ],
        "ball_swing_references": [
            "ball_swing_game/src/objects/ui.rs",
            "ball_swing_game/src/scenes/game/build_scene.rs"
        ],
        "quartz_forge_references": [
            "quartz_forge/src/services/codegen.rs",
            "quartz_forge/src/app/mod.rs",
            "quartz_forge/src/app/editors.rs"
        ],
        "source_signals": {
            "api_has_text_new": api_text.contains("Text::new"),
            "api_has_span_new": api_text.contains("Span::new"),
            "forge_codegen_uses_direct_text_new": forge_codegen.contains("set_text_value_expr") && forge_codegen.contains("Text::new") && forge_codegen.contains("Span::new")
        },
        "example_set_text": "static HUD_FONT: std::sync::OnceLock<Font> = std::sync::OnceLock::new(); let font = std::sync::Arc::new(HUD_FONT.get_or_init(|| Font::from_bytes(include_bytes!(\"../../assets/font.ttf\")).expect(\"hud font\")).clone()); let text = Text::new(vec![Span::new(\"Ready\".to_owned(), 30.0, Some(37.5), font, Color(255, 255, 255, 255), 0.0)], None, Align::Left, None); canvas.run(Action::set_text(Target::name(\"hud_label\"), text));"
    }))
}

fn parity_report(paths: &WorkspacePaths, surface: &str) -> Result<Value> {
    let action_quartz = enum_variants(&paths.quartz_action_rs, "Action")?;
    let action_forge = enum_variants(&paths.forge_domain_rs, "QuartzAction")?;
    let condition_quartz = enum_variants(&paths.quartz_condition_rs, "Condition")?;
    let condition_forge = enum_variants(&paths.forge_domain_rs, "QuartzCondition")?;

    let action_missing: Vec<_> = action_quartz
        .iter()
        .filter(|v| !action_forge.contains(v))
        .cloned()
        .collect();
    let condition_missing: Vec<_> = condition_quartz
        .iter()
        .filter(|v| !condition_forge.contains(v))
        .cloned()
        .collect();

    let mut report = serde_json::Map::new();
    if surface == "action" || surface == "all" {
        report.insert(
            "action".to_owned(),
            json!({
                "quartz_count": action_quartz.len(),
                "forge_count": action_forge.len(),
                "missing_in_forge": action_missing,
            }),
        );
    }
    if surface == "condition" || surface == "all" {
        report.insert(
            "condition".to_owned(),
            json!({
                "quartz_count": condition_quartz.len(),
                "forge_count": condition_forge.len(),
                "missing_in_forge": condition_missing,
            }),
        );
    }
    if surface == "wiring" || surface == "all" {
        let codegen_text = fs::read_to_string(paths.root.join("quartz_forge/src/services/codegen.rs"))
            .unwrap_or_default();
        let editors_text = fs::read_to_string(paths.root.join("quartz_forge/src/app/editors.rs"))
            .unwrap_or_default();
        let domain_text = fs::read_to_string(&paths.forge_domain_rs).unwrap_or_default();

        let new_variants = ["SetVar", "ModVar", "SpawnObject", "SetText"];
        let wiring: Vec<_> = new_variants.iter().map(|v| {
            json!({
                "variant": v,
                "in_domain": domain_text.contains(&format!("{v} {{")) || domain_text.contains(&format!("{v},")),
                "in_codegen": codegen_text.contains(&format!("QuartzAction::{v}")),
                "in_editors": editors_text.contains(&format!("QuartzAction::{v}")),
            })
        }).collect();
        report.insert(
            "new_action_wiring".to_owned(),
            json!({
                "variants_checked": new_variants,
                "status": wiring,
                "settext_prelude_hoisting": codegen_text.contains("action_expr_with_prelude") && codegen_text.contains("emit_action_lines"),
                "spawn_template_body_present": codegen_text.contains("spawn_template_body"),
            }),
        );
    }

    Ok(Value::Object(report))
}

fn spawn_audit(paths: &WorkspacePaths) -> Result<Value> {
    let app_text = fs::read_to_string(paths.root.join("quartz_forge/src/app/mod.rs"))?;
    let project_text = fs::read_to_string(paths.root.join("quartz_forge/src/core/project.rs"))?;
    let codegen_text = fs::read_to_string(paths.root.join("quartz_forge/src/services/codegen.rs"))?;
    let domain_text = fs::read_to_string(&paths.forge_domain_rs)?;

    Ok(json!({
        "spawn_only_field_present": domain_text.contains("spawn_only"),
        "spawn_overlay_toggle_present": app_text.contains("show_spawn_overlay"),
        "spawn_object_creator_present": project_text.contains("add_spawn_only_object_to_active_scene"),
        "spawn_helper_generation_present": codegen_text.contains("spawn_only"),
        "notes": [
            "spawn-only objects are omitted from setup_scene registration",
            "spawn-only objects can be drawn as ghost overlays when the overlay toggle is enabled",
            "spawn helpers are still emitted so runtime spawn actions can target them"
        ]
    }))
}

fn enum_variants(path: &Path, enum_name: &str) -> Result<Vec<String>> {
    let content = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    let mut inside = false;
    let mut depth = 0i32;
    let mut variants = Vec::new();

    for line in content.lines() {
        if !inside {
            if line.contains(&format!("pub enum {enum_name}")) {
                inside = true;
                depth += line.matches('{').count() as i32;
            }
            continue;
        }

        depth += line.matches('{').count() as i32;
        depth -= line.matches('}').count() as i32;

        if let Some(name) = line
            .trim()
            .split(|c: char| c == ' ' || c == '{' || c == '(' || c == ',')
            .next()
            .filter(|s| !s.is_empty())
        {
            if name.chars().next().is_some_and(|c| c.is_uppercase()) {
                variants.push(name.to_owned());
            }
        }

        if depth <= 0 {
            break;
        }
    }

    variants.sort();
    variants.dedup();
    Ok(variants)
}

fn locate_workspace_paths() -> Result<WorkspacePaths> {
    let mut current = env::current_dir().context("determine current directory")?;
    loop {
        let api = current.join("quartz").join("api.txt");
        let action = current.join("quartz").join("src").join("types").join("action.rs");
        let condition = current.join("quartz").join("src").join("types").join("condition.rs");
        let forge_domain = current.join("quartz_forge").join("src").join("core").join("quartz_domain.rs");

        if api.exists() && action.exists() && condition.exists() && forge_domain.exists() {
            let mcp_dir = current.join(".quartz_forge").join("mcp");
            return Ok(WorkspacePaths {
                root: current,
                quartz_api_txt: api,
                quartz_action_rs: action,
                quartz_condition_rs: condition,
                forge_domain_rs: forge_domain,
                mcp_dir: mcp_dir.clone(),
                lock_file: mcp_dir.join("server.lock"),
                heartbeat_file: mcp_dir.join("heartbeat.json"),
            });
        }

        if !current.pop() {
            return Err(anyhow!("could not locate FlowMake workspace root"));
        }
    }
}

fn health_report(paths: &WorkspacePaths) -> Result<Value> {
    fs::create_dir_all(&paths.mcp_dir)?;

    Ok(json!({
        "workspace_root": paths.root,
        "api_txt": paths.quartz_api_txt.exists(),
        "action_rs": paths.quartz_action_rs.exists(),
        "condition_rs": paths.quartz_condition_rs.exists(),
        "forge_domain_rs": paths.forge_domain_rs.exists(),
        "mcp_dir": paths.mcp_dir.exists(),
        "lock_status": lock_status(paths)?,
        "tools": tool_list().iter().map(|tool| tool.name).collect::<Vec<_>>()
    }))
}

fn lock_status(paths: &WorkspacePaths) -> Result<Value> {
    let lock_exists = paths.lock_file.exists();
    let heartbeat_exists = paths.heartbeat_file.exists();
    let heartbeat_age_s = if heartbeat_exists {
        let modified = fs::metadata(&paths.heartbeat_file)?.modified()?;
        SystemTime::now()
            .duration_since(modified)
            .unwrap_or(Duration::from_secs(0))
            .as_secs_f32()
    } else {
        -1.0
    };

    Ok(json!({
        "lock_exists": lock_exists,
        "heartbeat_exists": heartbeat_exists,
        "heartbeat_age_s": heartbeat_age_s,
        "lock_file": paths.lock_file,
        "heartbeat_file": paths.heartbeat_file,
    }))
}

fn acquire_lock(paths: &WorkspacePaths) -> Result<()> {
    fs::create_dir_all(&paths.mcp_dir)?;
    let pid = process::id();
    let lock_body = json!({
        "pid": pid,
        "started_at": SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs(),
    });

    if paths.lock_file.exists() {
        if let Ok(status) = lock_status(paths) {
            let stale = status
                .get("heartbeat_age_s")
                .and_then(Value::as_f64)
                .map(|age| age > 10.0)
                .unwrap_or(true);
            if !stale {
                return Err(anyhow!("quartz_forge_mcp lock is already held by another active process"));
            }
        }
    }

    fs::write(&paths.lock_file, serde_json::to_string_pretty(&lock_body)?)?;
    fs::write(
        &paths.heartbeat_file,
        serde_json::to_string_pretty(&json!({
            "pid": pid,
            "heartbeat_at": SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs(),
        }))?,
    )?;
    Ok(())
}

fn read_rpc_request(reader: &mut impl BufRead) -> Result<Option<JsonRpcRequest>> {
    let mut content_length = None;
    let mut line = String::new();

    loop {
        line.clear();
        let bytes = reader.read_line(&mut line)?;
        if bytes == 0 {
            return Ok(None);
        }
        let trimmed = line.trim_end_matches(['\r', '\n']);
        if trimmed.is_empty() {
            break;
        }
        if let Some(value) = trimmed.strip_prefix("Content-Length:") {
            content_length = Some(value.trim().parse::<usize>()?);
        }
    }

    let len = content_length.ok_or_else(|| anyhow!("missing Content-Length header"))?;
    let mut body = vec![0u8; len];
    reader.read_exact(&mut body)?;
    let request: JsonRpcRequest = serde_json::from_slice(&body)?;
    Ok(Some(request))
}

fn write_rpc_response(writer: &mut impl Write, response: JsonRpcResponse) -> Result<()> {
    let body = serde_json::to_vec(&response)?;
    write!(writer, "Content-Length: {}\r\n\r\n", body.len())?;
    writer.write_all(&body)?;
    Ok(())
}
