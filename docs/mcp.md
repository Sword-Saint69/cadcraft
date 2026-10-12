# CADCraft and AI agents (MCP)

CADCraft speaks the [Model Context Protocol](https://modelcontextprotocol.io) over stdio.
Any MCP client — Claude Code, Claude Desktop, VS Code, or your own harness —
can draft, edit, dimension, inspect, and render drawings, either headless
(in-process, no window) or bridged to the running app (watch the agent draw).

## Quick start: install as a Claude Code plugin

One install wires the MCP server, a drafting skill, and the `/cadcraft:status`
+ `/cadcraft:draft` commands (detail: `plugins/cadcraft/README.md`):

```sh
claude plugin marketplace add storytold/cadcraft
claude plugin install cadcraft@cadcraft-marketplace
```

Then just talk to Claude: "draw a 100x60 plate with four 8mm holes" or
`/cadcraft:draft a 100x60 plate with four 8mm holes`.

The plugin starts a **headless** session by default. To let Claude drive the
**live window** instead, start the app with `cadcraft --control 7979` and set
the plugin's `control_port` option to `7979` (`/config` → `cadcraft`).

## Setup without the plugin

You need the `cadcraft-cli` binary on `PATH` (`cargo install --path
apps/cadcraft-cli`, or a release package). Two modes:

```sh
# Headless: an in-process drawing session, no window.
cadcraft-cli mcp

# Bridged to the running app (watch the agent draw):
cadcraft --control 7979 &
cadcraft-cli mcp --connect 127.0.0.1:7979
```

`--connect` needs the app's `HOST:PORT`. Without one (or with an unknown
argument) `cadcraft-cli mcp` exits with an error instead of falling back to a
headless session, so a client meant for the app never edits a separate
drawing by mistake. If the app can't be reached, startup fails too.

Client configuration:

```json
// Claude Code / Claude Desktop / generic client, headless:
{ "mcpServers": { "cadcraft": { "command": "cadcraft-cli", "args": ["mcp"] } } }

// Bridged to a running app (also works in VS Code `mcpServers` settings):
{ "mcpServers": { "cadcraft": { "command": "cadcraft-cli", "args": ["mcp", "--connect", "127.0.0.1:7979"] } } }
```

Run `cadcraft-cli mcp --help` to print the full tool list with input schemas.

## How to work with it

There are two ways to draw. **Command line** (`command_line`) types exactly
like a person at an AutoCAD-style prompt — fastest for everyday drafting.
**JSON** (`execute`) runs any command with parameters and never opens a
dialog — best for exact values. `list_commands` shows every id with its
parameter docs; `filter` narrows it (`{"filter": "dim"}`).

Coordinates are drawing units, y up. Points are `x,y`; `@dx,dy` is relative;
`@dist<angle` is polar; spaces act as Enter; empty text is Enter.

### Draw, edit, dimension

```
command_line {"text": "rectang 0,0 100,60"}        # outline, one call
execute {"command": "circle", "params": {"center": [10, 10], "radius": 4}}
execute {"command": "layer.new", "params": {"name": "Center", "color": "red", "current": true}}
command_line {"text": "line -5,30 105,30"}          # centre line on the red layer
command_line {"text": "dimlinear 0,0 100,0 50,-8"}  # dimension below the plate
```

Multi-line `TEXT` keeps prompting until an empty line, so end scripts with a
blank line: `script {"text": "TEXT 0,-10 5 0 PLATE 100x60\n\n"}`. `cancel`
is Escape, `press_enter` is Enter, and `cmdline_state` polls the prompt
(prompt text, running command, keywords, accepted input, history) without
typing anything.

### Verify everything

```
inspect_drawing {}                                  # counts, extents, layers, blocks, undo
query_entities {"type": "Circle", "limit": 10}      # filtered geometry with handles
query_entities {"window": [[0, 0], [50, 60]]}       # true geometry in a window…
query_entities {"window": [[0, 0], [50, 60]], "crossing": false}  # …or entirely inside it
render {}                                           # look at it (PNG, fit-to-extents)
```

`query_entities` reports `count` (every match) and `returned` (this page);
page with `offset`/`limit` (default 500). Always verify after drawing —
counts first, then a render.

### Files

```
new_drawing {"metric": true}     # fresh metric drawing
open {"path": "bracket.dxf"}     # DXF in
save {}                          # save back
save {"path": "out.pdf"}         # .svg/.png/.pdf exports instead (drawing keeps its name)
```

### Prompts

Shortcuts clients can offer directly: `draft {task, units?}` builds a guided
drafting brief from plain language ("a 100x60 plate with four 8mm holes"),
and `review` runs the full counts → spot-check → render review.

## Driving the live app (bridged mode only)

These tools appear only with `--connect`. Synthetic input queues real egui
events — poll `ui_inspect` or `ui_elements` before the next action.

```
screenshot {}                        # PNG of the window
ui_inspect {}                        # panels, canvas rect, view, cursor/snap, perf, focus
ui_elements {"role": "button"}       # find widgets (exact label/role/focused filters)
ui_menu_list {}                      # menu tree, then…
ui_menu_invoke {"command": "zoom.extents"}  # …click one like a user
ui_click {"x": 50, "y": 30}          # drawing click, world coords (right button = Enter)
ui_click_screen {"x": 400, "y": 200} # real widget click, screen points
ui_move {"x": 400, "y": 200}         # hover / move
ui_drag {"x": 400, "y": 100, "to": [400, 160]}  # drag (command-line edge, ViewCube…)
ui_key {"key": "enter"}              # key press (egui names; cmd/shift/alt/ctrl modifiers)
ui_text {"text": "Walls"}            # type text
ui_set {"theme": "dark"}             # palettes, theme, saveFormat, saveVersion…
ui_resize {"width": 1600, "height": 1000}
ui_focus {}                          # bring the window forward
quit {}                              # quit without the save prompt (has destructiveHint)
```

## Errors, and what they mean

- **JSON-RPC `-32602` (invalid params):** unknown tool, `arguments` not an
  object, missing/wrong-typed argument. `command_line {}` never presses Enter
  by accident — it is rejected here.
- **`isError: true` result:** the tool ran and the drawing refused it (bad
  point, unknown command, bad layer). Read the message, fix the input.
- **Lost reply on a mutation** (`may or may not have been applied`): the
  request reached the app but the reply was lost. `inspect_drawing` first —
  never blindly resend, or you get two circles. Read-only tools
  (`inspect_drawing`, `query_entities`, `cmdline_state`, …) are safe to retry.
- **Timeout:** `not-run` means it never started (safe to resend);
  `may-have-run` means inspect first.

## Troubleshooting

| Symptom | Fix |
|---|---|
| Client can't start the server | `cadcraft-cli` not on `PATH`, or (plugin) set `CADCRAFT_CLI` to its full path |
| Bridged client edits nothing visible | App wasn't started with `--control PORT`, or the port mismatches `--connect` / `control_port` |
| `no UI in headless mode; use render` | That tool needs `--connect`; headless has no window — use `render` instead of `screenshot` |
| `unknown method` on old app builds | Update the app: new MCP tools need a matching control channel |
| Prompt stuck mid-command | `cmdline_state` to see what it waits for, answer it, or `cancel` |

## Tools reference

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

Prompts: `draft {task, units?}` (guided drafting from plain language) and
`review` (counts, spot-checks, render). Unknown prompts and a `draft` without
`task` are JSON-RPC errors `-32602`.

Arguments are checked against each tool's input schema. An unknown tool, `arguments` that aren't an
object, a missing required argument or one of the wrong type is a JSON-RPC error `-32602` (invalid
params), so `command_line {}` never presses Enter by accident. A tool that runs and fails (a bad
point, an unknown command) returns a result with `isError: true` and the message.
Read-only tools (`list_commands`, `inspect_drawing`, `query_entities`,
`cmdline_state`, `ui_inspect`, `ui_elements`, `ui_menu_list`) carry
`readOnlyHint`, and `quit` carries `destructiveHint`, so clients can show
confirmations where they matter.

## A typical session

1. `command_line {"text": "rectang 0,0 100,60"}`
2. `execute {"command": "circle", "params": {"center": [10, 10], "radius": 4}}`
3. `command_line {"text": "dimlinear 0,0 100,0 50,-8"}`
4. `inspect_drawing {}` — verify counts and geometry.
5. `render {}` — look at it.

The end-to-end test in `crates/mcp/src/tests.rs` runs a session like this and checks the result
only through MCP.
