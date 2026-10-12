//! An agent completes realistic drafting tasks through MCP only, and verifies through MCP only.

use serde_json::{Value, json};

use crate::{Headless, Remote, Server};

fn call(s: &mut Server, id: u64, method: &str, params: Value) -> Value {
    let line = json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}).to_string();
    let r = s.handle_line(&line).expect("reply");
    serde_json::from_str(&r).unwrap()
}

fn tool(s: &mut Server, name: &str, args: Value) -> Value {
    let r = call(s, 99, "tools/call", json!({"name": name, "arguments": args}));
    let c = &r["result"]["content"][0];
    assert_ne!(r["result"]["isError"], json!(true), "{name} failed: {c}");
    if c["type"] == "text" { serde_json::from_str(c["text"].as_str().unwrap()).unwrap_or(Value::Null) } else { c.clone() }
}

#[test]
fn lifecycle_and_tools() {
    let mut s = Server::new(Box::new(Headless::default()));
    let init = call(&mut s, 1, "initialize", json!({"protocolVersion": "2025-06-18"}));
    assert_eq!(init["result"]["serverInfo"]["name"], "cadcraft");
    assert!(s.handle_line(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#).is_none());
    assert!(s.is_initialized());
    let tools = call(&mut s, 2, "tools/list", json!({}));
    assert!(tools["result"]["tools"].as_array().unwrap().iter().any(|t| t["name"] == "command_line"));
    let bad = call(&mut s, 3, "nope", json!({}));
    assert_eq!(bad["error"]["code"], -32601);
}

#[test]
fn agent_draws_a_plate_with_holes() {
    let mut s = Server::new(Box::new(Headless::default()));
    // Task 1: an outline via the command line, AutoCAD style.
    tool(&mut s, "command_line", json!({"text": "rectang 0,0 100,60"}));
    // Task 2: four holes with JSON.
    for (x, y) in [(10, 10), (90, 10), (10, 50), (90, 50)] {
        tool(&mut s, "execute", json!({"command": "circle", "params": {"center": [x, y], "radius": 4}}));
    }
    // Task 3: a layer and a centre line on it.
    tool(&mut s, "execute", json!({"command": "layer.new", "params": {"name": "Center", "color": "red", "current": true}}));
    tool(&mut s, "command_line", json!({"text": "line -5,30 105,30"}));
    tool(&mut s, "command_line", json!({"text": ""}));
    // Task 4: a script with text. TEXT keeps asking for further lines until an empty one, as in
    // AutoCAD, so the script ends it with a blank line (= Enter).
    let r = tool(&mut s, "script", json!({"text": "TEXT 0,-10 5 0 PLATE 100x60\n\n"}));
    assert_eq!(r["state"]["running"], Value::Null, "TEXT must be finished: {r}");
    // Verify only through MCP.
    let d = tool(&mut s, "inspect_drawing", json!({}));
    assert_eq!(d["counts"]["Circle"], 4);
    assert_eq!(d["counts"]["Polyline"], 1);
    assert_eq!(d["counts"]["Line"], 1);
    assert_eq!(d["counts"]["Text"], 1);
    let lines = tool(&mut s, "query_entities", json!({"type": "Line", "layer": "Center"}));
    assert_eq!(lines["count"], 1);
    // Task 5: undo the text, check, then render.
    let u = tool(&mut s, "execute", json!({"command": "undo"}));
    assert_eq!(u["undone"], json!(["Single Line Text"]), "{u}");
    let d = tool(&mut s, "inspect_drawing", json!({"entities": false}));
    assert!(d["counts"].get("Text").is_none());
    assert_eq!(d["counts"]["Circle"], 4);
    assert_eq!(d["counts"]["Polyline"], 1);
    assert_eq!(d["counts"]["Line"], 1);
    let lines = tool(&mut s, "query_entities", json!({"type": "Line", "layer": "Center"}));
    assert_eq!(lines["count"], 1, "the centre line must survive undoing the text");
    let img = tool(&mut s, "render", json!({"width": 320, "height": 200}));
    assert_eq!(img["type"], "image");
    assert!(img["data"].as_str().unwrap().len() > 100);
}

#[test]
fn errors_are_tool_errors_not_crashes() {
    let mut s = Server::new(Box::new(Headless::default()));
    let r = call(&mut s, 1, "tools/call", json!({"name": "execute", "arguments": {"command": "circle", "params": {"center": "x"}}}));
    assert_eq!(r["result"]["isError"], true);
    // Every UI-only tool headless is a tool error telling the agent to use render instead.
    for (i, name) in [
        "screenshot",
        "ui_inspect",
        "ui_elements",
        "ui_menu_list",
        "ui_key",
        "ui_text",
        "ui_click_screen",
        "ui_move",
        "ui_drag",
        "ui_set",
        "ui_resize",
        "ui_focus",
        "quit",
    ]
    .iter()
    .enumerate()
    {
        let args = match *name {
            "ui_menu_invoke" => json!({"command": "x"}),
            "ui_key" => json!({"key": "a"}),
            "ui_text" => json!({"text": "a"}),
            "ui_click" | "ui_click_screen" | "ui_move" => json!({"x": 1, "y": 2}),
            "ui_drag" => json!({"x": 1, "y": 2, "to": [3, 4]}),
            "ui_resize" => json!({"width": 800, "height": 600}),
            _ => json!({}),
        };
        let r = call(&mut s, 10 + i as u64, "tools/call", json!({"name": name, "arguments": args}));
        assert_eq!(r["result"]["isError"], true, "{name} must be a tool error: {r}");
    }
    assert!(s.handle_line("{not json").unwrap().contains("-32700"));
}

/// Unknown tools and arguments that don't fit a tool's schema are invalid params (-32602), never
/// guessed at; an empty batch is one Invalid Request.
#[test]
fn bad_tool_calls_are_invalid_params() {
    let mut s = Server::new(Box::new(Headless::default()));
    tool(&mut s, "command_line", json!({"text": "circle 0,0 1"}));
    let bad = [
        json!({"name": "nope", "arguments": {}}),
        json!({"name": "command_line", "arguments": {}}),
        json!({"name": "command_line", "arguments": {"text": 5}}),
        json!({"name": "command_line", "arguments": "circle 1,1 1"}),
        json!({"name": "script", "arguments": {}}),
        json!({"name": "query_entities", "arguments": {"window": [-5, -5, 5, 5]}}),
        json!({"name": "query_entities", "arguments": {"window": [[-5, -5]]}}),
        json!({"name": "query_entities", "arguments": {"window": {"min": [-5, -5], "max": [5, 5]}}}),
        json!({"name": "query_entities", "arguments": {"type": 7}}),
        json!({"name": "query_entities", "arguments": {"layer": ["0"]}}),
        json!({"name": "query_entities", "arguments": {"offset": -1}}),
        json!({"name": "ui_click", "arguments": {"x": 1}}),
        json!({"name": "ui_click", "arguments": {"x": 1, "y": 2, "space": "galaxy"}}),
        json!({"name": "ui_click", "arguments": {"x": 1, "y": 2, "button": "middle"}}),
        json!({"name": "ui_click_screen", "arguments": {"x": 1, "y": 2, "button": "double"}}),
        json!({"name": "ui_drag", "arguments": {"x": 1, "y": 2}}),
        json!({"name": "ui_drag", "arguments": {"x": 1, "y": 2, "to": [3]}}),
        json!({"name": "ui_key", "arguments": {}}),
        json!({"name": "ui_key", "arguments": {"key": 5}}),
        json!({"name": "ui_text", "arguments": {}}),
        json!({"name": "ui_menu_invoke", "arguments": {}}),
        json!({"name": "ui_resize", "arguments": {"width": 800}}),
        json!({"name": "ui_elements", "arguments": {"focused": "yes"}}),
    ];
    for (i, params) in bad.into_iter().enumerate() {
        let r = call(&mut s, i as u64, "tools/call", params.clone());
        assert_eq!(r["error"]["code"], -32602, "{params} -> {r}");
    }
    // Nothing was typed: the circle is still the only object and no command is running.
    let d = tool(&mut s, "inspect_drawing", json!({"entities": false}));
    assert_eq!(d["entityCount"], 1, "{d}");
    assert_eq!(s.handle_line("[]").map(|r| serde_json::from_str::<Value>(&r).unwrap()["error"]["code"].clone()), Some(json!(-32600)));
}

/// `count` is every match while `returned` is the page; a window matches geometry, not bounding boxes.
#[test]
fn query_entities_pages_and_windows_by_geometry() {
    let mut s = Server::new(Box::new(Headless::default()));
    for c in ["circle 0,0 1", "circle 100,100 1", "circle 200,0 1"] {
        tool(&mut s, "command_line", json!({"text": c}));
    }
    // An arc whose bounding box covers the window but whose curve stays outside it.
    tool(&mut s, "execute", json!({"command": "arc", "params": {"p1": [40, 0], "p2": [50, 10], "p3": [60, 0]}}));
    let r = tool(&mut s, "query_entities", json!({"type": "Circle", "limit": 1, "offset": 1}));
    assert_eq!((r["count"].clone(), r["returned"].clone()), (json!(3), json!(1)), "{r}");
    assert_eq!(r["entities"][0]["geometry"]["center"]["x"], 100.0, "{r}");
    let r = tool(&mut s, "query_entities", json!({"window": [[45, 1], [55, 5]]}));
    assert_eq!(r["count"], 0, "the arc passes above this window: {r}");
    let r = tool(&mut s, "query_entities", json!({"window": [[-5, -5], [55, 5]]}));
    assert_eq!(r["count"], 2, "the circle at 0,0 and the arc crossing the window: {r}");
    let r = tool(&mut s, "query_entities", json!({"window": [[-5, -5], [55, 5]], "crossing": false}));
    assert_eq!(r["count"], 1, "only the circle is entirely inside: {r}");
}

#[test]
fn list_commands_filter_matches_names_not_docs() {
    let mut s = Server::new(Box::new(Headless::default()));
    let r = tool(&mut s, "list_commands", json!({"filter": "circle"}));
    let ids: Vec<&str> = r.as_array().unwrap().iter().filter_map(|c| c["id"].as_str()).collect();
    assert!(ids.contains(&"circle"), "{ids:?}");
    assert!(!ids.contains(&"qselect"), "QSELECT only mentions circles in its docs: {ids:?}");
    let r = tool(&mut s, "list_commands", json!({"filter": "pl"}));
    assert!(r.as_array().unwrap().iter().any(|c| c["id"] == "pline"), "aliases and ids match");
}

#[test]
fn cmdline_state_and_enter_round_trip() {
    let mut s = Server::new(Box::new(Headless::default()));
    let st = tool(&mut s, "cmdline_state", json!({}));
    assert_eq!(st["prompt"], "Command:", "{st}");
    assert_eq!(st["running"], Value::Null, "{st}");
    tool(&mut s, "command_line", json!({"text": "line 0,0 10,0"}));
    let st = tool(&mut s, "cmdline_state", json!({}));
    assert!(st["running"].is_string(), "LINE waits for more points: {st}");
    let st = tool(&mut s, "press_enter", json!({}));
    assert!(st.get("output").is_some(), "enter replies with prompt state: {st}");
    tool(&mut s, "cancel", json!({}));
    let st = tool(&mut s, "cmdline_state", json!({}));
    assert_eq!(st["running"], Value::Null, "{st}");
}

#[test]
fn headless_tool_list_hides_ui_tools() {
    let mut s = Server::new(Box::new(Headless::default()));
    let tools = call(&mut s, 2, "tools/list", json!({}));
    let names: Vec<&str> = tools["result"]["tools"].as_array().unwrap().iter().filter_map(|t| t["name"].as_str()).collect();
    for n in ["command_line", "execute", "cmdline_state", "press_enter", "cancel", "render"] {
        assert!(names.contains(&n), "{n} must be listed headless: {names:?}");
    }
    for n in ["screenshot", "ui_inspect", "ui_elements", "ui_menu_list", "ui_key", "quit"] {
        assert!(!names.contains(&n), "{n} must be hidden headless: {names:?}");
    }
}

#[test]
fn ui_set_accepts_free_form_object() {
    // check_args must not reject unknown UiState keys: validation happens in the app.
    let mut s = Server::new(Box::new(Headless::default()));
    let r = call(&mut s, 1, "tools/call", json!({"name": "ui_set", "arguments": {"theme": "dark"}}));
    // Headless has no UI, so it is a tool error — but never invalid params (-32602).
    assert_eq!(r["result"]["isError"], true, "{r}");
}

#[test]
fn prompts_list_and_get() {
    use crate::tools::tool_definitions;
    let mut s = Server::new(Box::new(Headless::default()));
    let r = call(&mut s, 1, "prompts/list", json!({}));
    let names: Vec<&str> = r["result"]["prompts"].as_array().unwrap().iter().filter_map(|p| p["name"].as_str()).collect();
    assert!(names.contains(&"draft") && names.contains(&"review"), "{names:?}");
    let r = call(&mut s, 2, "prompts/get", json!({"name": "draft", "arguments": {"task": "a 100x60 plate"}}));
    let text = r["result"]["messages"][0]["content"]["text"].as_str().unwrap_or("");
    assert!(text.contains("100x60 plate"), "{r}");
    let r = call(&mut s, 3, "prompts/get", json!({"name": "review"}));
    assert_eq!(r["result"]["messages"][0]["role"], "user");
    // Unknown prompt and missing required argument are invalid params, never tool errors.
    for (i, params) in [json!({"name": "nope"}), json!({"name": "draft"}), json!({"name": "draft", "arguments": {}})].into_iter().enumerate() {
        let r = call(&mut s, 10 + i as u64, "prompts/get", params.clone());
        assert_eq!(r["error"]["code"], -32602, "{params} -> {r}");
    }
    // Readers carry readOnlyHint, quit carries destructiveHint.
    let defs = tool_definitions(true);
    let ann = |tool: &str| {
        defs["tools"].as_array().unwrap().iter().find(|t| t["name"] == tool).and_then(|t| t.get("annotations")).cloned().unwrap_or(Value::Null)
    };
    assert_eq!(ann("inspect_drawing"), json!({"readOnlyHint": true}));
    assert_eq!(ann("ui_menu_list"), json!({"readOnlyHint": true}));
    assert_eq!(ann("quit"), json!({"destructiveHint": true}));
    assert!(ann("execute").is_null() && ann("render").is_null());
}

/// Every tool routes to its control-channel method: a fake app records what arrives.
#[test]
fn bridged_tools_route_to_expected_control_methods() {
    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpListener;
    use std::sync::mpsc;

    let cases: Vec<(&str, Value, &str)> = vec![
        ("command_line", json!({"text": "circle 0,0 1"}), "cmdline.input"),
        ("script", json!({"text": "circle 0,0 1"}), "cmdline.script"),
        ("cmdline_state", json!({}), "cmdline.state"),
        ("press_enter", json!({}), "cmdline.key"),
        ("cancel", json!({}), "cmdline.key"),
        ("execute", json!({"command": "undo"}), "engine.execute"),
        ("list_commands", json!({}), "engine.commands"),
        ("inspect_drawing", json!({}), "drawing.inspect"),
        ("query_entities", json!({"type": "Circle"}), "engine.execute"),
        ("new_drawing", json!({}), "engine.execute"),
        ("open", json!({"path": "a.dxf"}), "app.open"),
        ("save", json!({}), "app.save"),
        ("render", json!({}), "ui.render"),
        ("screenshot", json!({}), "ui.screenshot"),
        ("ui_inspect", json!({}), "ui.inspect"),
        ("ui_click", json!({"x": 1, "y": 2}), "ui.pointer"),
        ("ui_elements", json!({}), "ui.elements"),
        ("ui_menu_list", json!({}), "ui.menu.list"),
        ("ui_menu_invoke", json!({"command": "zoom.extents"}), "ui.menu.invoke"),
        ("ui_key", json!({"key": "a"}), "ui.key"),
        ("ui_text", json!({"text": "hi"}), "ui.text"),
        ("ui_click_screen", json!({"x": 1, "y": 2}), "ui.click"),
        ("ui_move", json!({"x": 1, "y": 2}), "ui.move"),
        ("ui_drag", json!({"x": 1, "y": 2, "to": [3, 4]}), "ui.drag"),
        ("ui_set", json!({"theme": "dark"}), "ui.set"),
        ("ui_resize", json!({"width": 800, "height": 600}), "ui.resize"),
        ("ui_focus", json!({}), "ui.focus"),
        ("quit", json!({}), "app.quit"),
    ];
    let n = cases.len();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap().to_string();
    let (tx, rx) = mpsc::channel();
    let server = std::thread::spawn(move || {
        let (conn, _) = listener.accept().unwrap();
        let mut reader = BufReader::new(conn.try_clone().unwrap());
        let mut writer = conn;
        for _ in 0..n {
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            let req: Value = serde_json::from_str(&line).unwrap();
            tx.send(req["method"].as_str().unwrap().to_string()).unwrap();
            writeln!(writer, "{}", json!({"id": req["id"].clone(), "ok": true, "result": {}})).unwrap();
        }
    });
    let mut s = Server::new(Box::new(Remote::connect(&addr).unwrap()));
    for (tool_name, args, _want) in &cases {
        let r = call(&mut s, 99, "tools/call", json!({"name": tool_name, "arguments": args}));
        assert!(r.get("error").is_none(), "{tool_name} failed: {r}");
        assert_ne!(r["result"]["isError"], json!(true), "{tool_name} failed: {r}");
    }
    server.join().unwrap();
    let got: Vec<String> = rx.try_iter().collect();
    let want: Vec<String> = cases.iter().map(|c| c.2.to_string()).collect();
    assert_eq!(got, want);
}
