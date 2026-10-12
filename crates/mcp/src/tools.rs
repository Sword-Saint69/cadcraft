//! MCP tool definitions and dispatch.

use serde_json::{Value, json};

use crate::backend::Backend;

pub fn tool_definitions(has_ui: bool) -> Value {
    let mut tools = vec![
        json!({"name": "command_line", "description": "Type one line at the CADCraft command line, exactly as a user would at an AutoCAD-style prompt. Starts commands by name or alias (LINE, L, CIRCLE, C, PLINE, RECTANG, MOVE, TRIM, OFFSET, ZOOM…) and answers the active prompt: points `x,y`, relative `@dx,dy`, polar `@dist<angle`, distances, keywords (e.g. `c` for Close), empty text = Enter. Spaces act as Enter, so `circle 0,0 5` works in one call. Returns the new prompt and command-line output, and `error` when the line was refused.",
            "inputSchema": {"type": "object", "properties": {"text": {"type": "string"}}, "required": ["text"]}}),
        json!({"name": "script", "description": "Run a multi-line script (like an AutoCAD .scr file): one or more inputs per line, blank line = Enter.",
            "inputSchema": {"type": "object", "properties": {"text": {"type": "string"}}, "required": ["text"]}}),
        json!({"name": "execute", "description": "Run any command with JSON parameters (no prompts or dialogs). Use list_commands to find ids and parameter docs. Examples: line {points:[[0,0],[10,0]]}, circle {center:[0,0], radius:5}, layer.new {name:\"Walls\", color:\"red\", current:true}, move {handles:[\"10A\"], delta:[5,0]}, zoom {mode:\"extents\"}.",
            "inputSchema": {"type": "object", "properties": {"command": {"type": "string"}, "params": {"type": "object"}}, "required": ["command"]}}),
        json!({"name": "list_commands", "description": "List commands with ids, labels, menu paths, aliases and parameter docs. Optional filter: a substring of the command id, label or an alias.",
            "inputSchema": {"type": "object", "properties": {"filter": {"type": "string"}}}}),
        json!({"name": "inspect_drawing", "description": "Summary of the active drawing: entity counts, extents, layers, styles, blocks, layouts, selection, undo history and (optionally) entities with handles and geometry.",
            "inputSchema": {"type": "object", "properties": {"entities": {"type": "boolean"}, "limit": {"type": "integer"}}}}),
        json!({"name": "query_entities", "description": "Entities filtered by type (Line, Circle, Polyline…), layer and/or a window [[x0,y0],[x1,y1]] (objects whose geometry is inside or crosses it; crossing:false = entirely inside). Returns count (all matches), returned (this page) and entities; page with offset and limit (default 500).",
            "inputSchema": {"type": "object", "properties": {"type": {"type": "string"}, "layer": {"type": "string"},
                "window": {"type": "array", "items": {"type": "array", "items": {"type": "number"}, "minItems": 2, "maxItems": 2}, "minItems": 2, "maxItems": 2},
                "crossing": {"type": "boolean"}, "limit": {"type": "integer", "minimum": 0}, "offset": {"type": "integer", "minimum": 0}}}}),
        json!({"name": "render", "description": "Render the active drawing to a PNG (fitted to extents by default) and return it as an image.",
            "inputSchema": {"type": "object", "properties": {"width": {"type": "integer"}, "height": {"type": "integer"}, "fit": {"type": "boolean"}, "path": {"type": "string"}}}}),
        json!({"name": "new_drawing", "description": "Create a new drawing (imperial by default, or metric).",
            "inputSchema": {"type": "object", "properties": {"metric": {"type": "boolean"}}}}),
        json!({"name": "open", "description": "Open a DXF file.", "inputSchema": {"type": "object", "properties": {"path": {"type": "string"}}, "required": ["path"]}}),
        json!({"name": "save", "description": "Save the drawing (DXF or DWG by extension). A .svg, .png or .pdf path exports instead: the drawing keeps its name and unsaved changes.", "inputSchema": {"type": "object", "properties": {"path": {"type": "string"}}}}),
        json!({"name": "cancel", "description": "Press Escape: cancel the running command (or clear the selection).", "inputSchema": {"type": "object", "properties": {}}}),
        json!({"name": "cmdline_state", "description": "Current command-line state: prompt text, running command, keywords, accepted input kinds and history tail. Read-only poll; never changes the drawing.",
            "inputSchema": {"type": "object", "properties": {}}}),
        json!({"name": "press_enter", "description": "Press Enter at the command line: accept the default, finish the command or repeat the last one. A refused Enter is reported as isError with the message.",
            "inputSchema": {"type": "object", "properties": {}}}),
    ];
    if has_ui {
        tools.push(json!({"name": "screenshot", "description": "Screenshot the CADCraft window (PNG).", "inputSchema": {"type": "object", "properties": {"path": {"type": "string"}}}}));
        tools.push(json!({"name": "ui_inspect", "description": "UI state: panels, canvas rect, view, cursor and snap, performance.", "inputSchema": {"type": "object", "properties": {}}}));
        tools.push(json!({"name": "ui_click", "description": "Click in the drawing area at world coordinates (or screen with space:\"screen\"). Right button = Enter.", "inputSchema": {"type": "object", "properties": {"x": {"type": "number"}, "y": {"type": "number"}, "space": {"type": "string", "enum": ["world", "screen"]}, "button": {"type": "string", "enum": ["left", "right"]}}, "required": ["x", "y"]}}));
        tools.push(json!({"name": "ui_elements", "description": "Observed toolbar, tab, drafting and layer-palette controls from the latest UI frame. Filters match exactly. Each entry has its real egui id, label, role, rectangle, enabled/visible/selected state and focus.",
            "inputSchema": {"type": "object", "properties": {"label": {"type": "string"}, "role": {"type": "string"}, "focused": {"type": "boolean"}}}}));
        tools.push(json!({"name": "ui_menu_list", "description": "The menu tree (menus with children items: label, command, shortcut, enabled).",
            "inputSchema": {"type": "object", "properties": {}}}));
        tools.push(json!({"name": "ui_menu_invoke", "description": "Invoke a menu item like a click (interactive prompt sequence starts). Use ui_menu_list to find the command id.",
            "inputSchema": {"type": "object", "properties": {"command": {"type": "string"}}, "required": ["command"]}}));
        tools.push(json!({"name": "ui_key", "description": "Synthetic keyboard key press (queued as real egui key events). Poll ui_inspect or ui_elements for the result. Key names follow egui (a-z, 0-9, enter, escape, tab, space, delete, backspace, arrows…).",
            "inputSchema": {"type": "object", "properties": {"key": {"type": "string"}, "cmd": {"type": "boolean"}, "shift": {"type": "boolean"}, "alt": {"type": "boolean"}, "ctrl": {"type": "boolean"}}, "required": ["key"]}}));
        tools.push(json!({"name": "ui_text", "description": "Synthetic text input (queued as a real egui text event).",
            "inputSchema": {"type": "object", "properties": {"text": {"type": "string"}}, "required": ["text"]}}));
        tools.push(json!({"name": "ui_click_screen", "description": "Real egui click at screen points (reaches every widget, unlike ui_click which defaults to world).",
            "inputSchema": {"type": "object", "properties": {"x": {"type": "number"}, "y": {"type": "number"}, "button": {"type": "string", "enum": ["left", "right", "middle"]}, "shift": {"type": "boolean"}, "ctrl": {"type": "boolean"}}, "required": ["x", "y"]}}));
        tools.push(json!({"name": "ui_move", "description": "Real egui pointer move to screen points.",
            "inputSchema": {"type": "object", "properties": {"x": {"type": "number"}, "y": {"type": "number"}}, "required": ["x", "y"]}}));
        tools.push(json!({"name": "ui_drag", "description": "Real press, move and release from (x, y) to `to` in screen points (drags the command line edge, the ViewCube ring…).",
            "inputSchema": {"type": "object", "properties": {"x": {"type": "number"}, "y": {"type": "number"}, "to": {"type": "array", "items": {"type": "number"}, "minItems": 2, "maxItems": 2}, "button": {"type": "string", "enum": ["left", "right", "middle"]}}, "required": ["x", "y", "to"]}}));
        tools.push(json!({"name": "ui_set", "description": "Show/hide palettes, toolbars, command line, ViewCube…; theme system|light|dark; saveFormat dxf|dwg; saveVersion 2000…2018. Free-form UiState fields; invalid values return isError without changing layout.",
            "inputSchema": {"type": "object"}}));
        tools.push(json!({"name": "ui_resize", "description": "Resize the app window (screen points).",
            "inputSchema": {"type": "object", "properties": {"width": {"type": "number"}, "height": {"type": "number"}}, "required": ["width", "height"]}}));
        tools.push(json!({"name": "ui_focus", "description": "Activate the native app window. Does not assign focus to a widget.",
            "inputSchema": {"type": "object", "properties": {}}}));
        tools.push(json!({"name": "quit", "description": "Quit the app without the unsaved-changes prompt (programmatic quit never opens dialogs). Only when connected to the app.",
            "inputSchema": {"type": "object", "properties": {}}}));
    }
    json!({ "tools": tools })
}

fn text(v: &Value) -> Value {
    json!({"content": [{"type": "text", "text": serde_json::to_string_pretty(v).unwrap_or_default()}]})
}

fn err_text(e: &str) -> Value {
    json!({"content": [{"type": "text", "text": e}], "isError": true})
}

/// Whether `v` fits the subset of JSON Schema the tool definitions use: `type`, `enum`,
/// `items`, `minItems`, `maxItems` and `minimum`.
fn fits(v: &Value, schema: &Value) -> bool {
    let typed = match schema["type"].as_str() {
        Some("string") => v.is_string(),
        // JSON Schema integers include numbers such as 2.0.
        Some("integer") => v.as_f64().is_some_and(|f| f.fract() == 0.0),
        Some("number") => v.is_number(),
        Some("boolean") => v.is_boolean(),
        Some("array") => v.is_array(),
        Some("object") => v.is_object(),
        _ => true,
    };
    let in_enum = schema.get("enum").and_then(Value::as_array).is_none_or(|allowed| allowed.iter().any(|e| e == v));
    let len = v.as_array().map(Vec::len);
    typed
        && in_enum
        && schema["minimum"].as_f64().is_none_or(|m| v.as_f64().is_none_or(|f| f >= m))
        && schema["minItems"].as_u64().is_none_or(|m| len.is_none_or(|n| n as u64 >= m))
        && schema["maxItems"].as_u64().is_none_or(|m| len.is_none_or(|n| n as u64 <= m))
        && (schema["items"].is_null() || v.as_array().is_none_or(|a| a.iter().all(|i| fits(i, &schema["items"]))))
}

/// Arguments checked against the tool's input schema: an object, required properties present and
/// each given property fitting its schema (`null` counts as not given). `Err` is the
/// invalid-params message.
fn check_args(name: &str, args: &Value) -> Result<Value, String> {
    let defs = tool_definitions(true);
    let schema = defs["tools"].as_array().and_then(|t| t.iter().find(|t| t["name"] == name)).map(|t| t["inputSchema"].clone());
    let Some(schema) = schema else { return Err(format!("unknown tool `{name}`")) };
    let a = match args {
        Value::Null => json!({}),
        Value::Object(_) => args.clone(),
        _ => return Err(format!("`arguments` of `{name}` must be an object")),
    };
    // Schemas without `properties` (e.g. ui_set's free-form UiState object) accept any object.
    for key in schema["required"].as_array().into_iter().flatten().filter_map(Value::as_str) {
        if a.get(key).is_none_or(Value::is_null) {
            return Err(format!("`{name}` needs `{key}`"));
        }
    }
    let Some(props) = schema.get("properties").and_then(Value::as_object) else { return Ok(a) };
    for (key, prop) in props {
        if let Some(v) = a.get(key).filter(|v| !v.is_null())
            && !fits(v, prop)
        {
            return Err(format!("`{key}` of `{name}` must match {prop}"));
        }
    }
    Ok(a)
}

/// Whether a command-catalog entry's id, label or one of its aliases contains `f` (lower case).
fn command_matches(c: &Value, f: &str) -> bool {
    let has = |s: &str| s.to_ascii_lowercase().contains(f);
    ["id", "label"].iter().any(|k| c[*k].as_str().is_some_and(has))
        || c["aliases"].as_array().is_some_and(|a| a.iter().filter_map(Value::as_str).any(has))
}

/// Run a tool. `Err` is a protocol error (unknown tool, or arguments that don't fit its input
/// schema), answered with JSON-RPC invalid params; a failing tool is an `isError` result.
pub fn call_tool(b: &mut dyn Backend, name: &str, args: &Value) -> Result<Value, String> {
    let a = check_args(name, args)?;
    let r = match name {
        "command_line" => b.call("cmdline.input", a),
        "script" => b.call("cmdline.script", a),
        "execute" => b.call(
            "engine.execute",
            json!({"command": a.get("command").cloned().unwrap_or(Value::Null), "params": a.get("params").cloned().unwrap_or(json!({}))}),
        ),
        "list_commands" => b.call("engine.commands", json!({})).map(|v| {
            let f = a.get("filter").and_then(Value::as_str).map(str::to_ascii_lowercase);
            match (v, f) {
                (Value::Array(items), Some(f)) => Value::Array(items.into_iter().filter(|c| command_matches(c, &f)).collect()),
                (v, _) => v,
            }
        }),
        "inspect_drawing" => b.call("drawing.inspect", a),
        "query_entities" => b.call("engine.execute", json!({"command": "entities", "params": a})),
        "new_drawing" => b.call("engine.execute", json!({"command": "new", "params": a})),
        "open" => b.call("app.open", a),
        "save" => b.call("app.save", a),
        "cancel" => b.call("cmdline.key", json!({"key": "escape"})),
        "cmdline_state" => b.call("cmdline.state", json!({})),
        "press_enter" => b.call("cmdline.key", json!({"key": "enter"})),
        "screenshot" => b.call("ui.screenshot", a),
        "ui_inspect" => b.call("ui.inspect", json!({})),
        "ui_click" => b.call("ui.pointer", a),
        "ui_elements" => b.call("ui.elements", a),
        "ui_menu_list" => b.call("ui.menu.list", json!({})),
        "ui_menu_invoke" => b.call("ui.menu.invoke", a),
        "ui_key" => b.call("ui.key", a),
        "ui_text" => b.call("ui.text", a),
        "ui_click_screen" => b.call("ui.click", a),
        "ui_move" => b.call("ui.move", a),
        "ui_drag" => b.call("ui.drag", a),
        "ui_set" => b.call("ui.set", a),
        "ui_resize" => b.call("ui.resize", a),
        "ui_focus" => b.call("ui.focus", json!({})),
        "quit" => b.call("app.quit", json!({})),
        "render" => {
            return Ok(match b.call("ui.render", a) {
                Ok(v) => match v.get("pngBase64").and_then(Value::as_str) {
                    Some(data) => json!({"content": [{"type": "image", "data": data, "mimeType": "image/png"}]}),
                    None => text(&v),
                },
                Err(e) => err_text(&e),
            });
        }
        other => return Err(format!("unknown tool `{other}`")),
    };
    Ok(match r {
        Ok(v) => text(&v),
        Err(e) => err_text(&e),
    })
}
