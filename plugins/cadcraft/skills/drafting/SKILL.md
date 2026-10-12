---
name: drafting
description: Draft, edit, dimension, and verify 2D CAD drawings in CADCraft over MCP. Use when the user wants to draw, modify, inspect, or render a CAD drawing.
---

# CAD drafting with CADCraft

CADCraft is an AutoCAD-style 2D drafting app. Coordinates are drawing units, y up.

## Fastest path

Prefer `command_line` with AutoCAD syntax, one call per line. Spaces act as
Enter, so a full command fits in one call:

- `rectang 0,0 100,60`, `circle 10,10 4`, `line 0,0 @10,0 @0,5 c`
- relative points `@dx,dy`, polar `@dist<angle`, keywords (`c` = Close)
- empty text is Enter; `cancel` is Escape, `press_enter` is Enter

For exact parameters, use `execute` with JSON instead (`list_commands` shows
ids and parameter docs): `circle {center:[0,0], radius:5}`,
`layer.new {name:"Walls", color:"red", current:true}`.

## Always verify

1. `inspect_drawing {}` — counts, extents, layers, blocks, layouts, undo.
2. `query_entities {type?, layer?, window?}` — filtered geometry with handles.
   `window [[x0,y0],[x1,y1]]` matches true geometry, not bounding boxes.
3. `render {}` — look at the drawing. Fit-to-extents by default.

`cmdline_state` polls the prompt without typing. A tool that runs and fails
returns `isError: true` with the message — read it and fix the input.

## Bridged to the running app

When the plugin's `control_port` is set (app started with `--control PORT`),
extra UI tools appear: `screenshot`, `ui_inspect`, `ui_elements`,
`ui_menu_list` / `ui_menu_invoke`, `ui_click` (drawing world coords) /
`ui_click_screen` / `ui_move` / `ui_drag`, `ui_key` / `ui_text`, `ui_set` /
`ui_resize` / `ui_focus`, `quit`. Synthetic input queues real egui events —
poll `ui_inspect` or `ui_elements` before sending the next action.
