# CADCraft and AI agents (MCP)

CADCraft speaks the [Model Context Protocol](https://modelcontextprotocol.io) over stdio.

## Start it

```sh
# Headless: an in-process drawing session, no window.
cadcraft-cli mcp

# Bridged to the running app (watch the agent draw):
cadcraft --control 7979 &
cadcraft-cli mcp --connect 127.0.0.1:7979
```

`--connect` needs the app's `HOST:PORT`. Without one (or with an unknown argument) `cadcraft-cli mcp`
exits with an error instead of falling back to a headless session, so a client meant for the app
never edits a separate drawing by mistake. If the app can't be reached, startup fails too.

Example client configuration (Claude Code / Claude Desktop):

```json
{ "mcpServers": { "cadcraft": { "command": "cadcraft-cli", "args": ["mcp"] } } }
```

VS Code (`mcpServers` in settings, bridged to a running app):

```json
{ "mcpServers": { "cadcraft": { "command": "cadcraft-cli", "args": ["mcp", "--connect", "127.0.0.1:7979"] } } }
```

Run `cadcraft-cli mcp --help` to print the full tool list with input schemas.

## Tools

| Tool | What it does |
|---|---|
| `command_line {text}` | Type at the command line exactly like a person: `line 0,0 @10,0 @0,5 c`, `circle 5,2 1`, `offset 0.5`, `zoom e`. Spaces act as Enter; empty text is Enter. Returns the new prompt, keywords and output. |
| `script {text}` | A multi-line script (like `.scr` files). |
| `execute {command, params}` | Any command with JSON parameters, never a dialog. `list_commands` shows ids and parameter docs. |
| `list_commands {filter?}` | The command catalog; `filter` matches part of a command's id, label or an alias. |
| `inspect_drawing {entities?, limit?}` | Counts, extents, layers, styles, blocks, layouts, selection, undo history and entities. `query_entities` is the filtered/paged variant. |
| `query_entities {type?, layer?, window?, crossing?, limit?, offset?}` | Filtered entities with handles and geometry. `window` `[[x0,y0],[x1,y1]]` finds objects whose geometry is inside it or crosses it (`crossing: false`: entirely inside). `count` is every match, `returned` the page (`offset`, `limit`, default 500). |
| `render {width?, height?, fit?}` | A PNG image of the drawing. |
| `new_drawing {metric?}`, `open {path}`, `save {path?}` | Files. `save` to a `.svg`, `.png` or `.pdf` path exports: the drawing keeps its name and unsaved changes. |
| `cancel`, `press_enter`, `cmdline_state` | Escape, Enter, and read-only prompt poll (prompt, running command, keywords, accept, history). |
| `screenshot`, `ui_inspect` | Only when connected to the app. Window PNG; UI state (panels, canvas rect, view, cursor/snap, perf). |
| `ui_click {x, y, space?, button?}` | Click in the drawing area (world by default, `space:"screen"` for screen). Right button = Enter. App only. |
| `ui_elements {label?, role?, focused?}` | Observed widgets from the latest frame (id, label, role, rect, enabled/visible/selected, focus). App only. |
| `ui_menu_list`, `ui_menu_invoke {command}` | Menu tree; invoke an item like a click (interactive). App only. |
| `ui_key {key, cmd?, shift?, alt?, ctrl?}`, `ui_text {text}` | Synthetic keyboard input (queued egui events; poll `ui_inspect`/`ui_elements`). App only. |
| `ui_click_screen {x, y, button?, shift?, ctrl?}`, `ui_move {x, y}`, `ui_drag {x, y, to}` | Real egui pointer input in screen points. App only. |
| `ui_set {...}`, `ui_resize {width, height}`, `ui_focus` | Palettes/theme/saveFormat/saveVersion, window size, native focus. App only. |
| `quit` | Quit the app without the save prompt. App only. |

Resources: `cadcraft://drawing` (inspect JSON) and `cadcraft://commands` (catalog).

Arguments are checked against each tool's input schema. An unknown tool, `arguments` that aren't an
object, a missing required argument or one of the wrong type is a JSON-RPC error `-32602` (invalid
params), so `command_line {}` never presses Enter by accident. A tool that runs and fails (a bad
point, an unknown command) returns a result with `isError: true` and the message.

## A typical session

1. `command_line {"text": "rectang 0,0 100,60"}`
2. `execute {"command": "circle", "params": {"center": [10, 10], "radius": 4}}`
3. `command_line {"text": "dimlinear 0,0 100,0 50,-8"}`
4. `inspect_drawing {}` — verify counts and geometry.
5. `render {}` — look at it.

The end-to-end test in `crates/mcp/src/tests.rs` runs a session like this and checks the result
only through MCP.
