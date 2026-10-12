---
description: Draft something in CADCraft from a plain-language description.
argument-hint: [what to draw]
---

Draft what the user asked for ($ARGUMENTS) in the active CADCraft drawing:

1. If the request is ambiguous (units, size, position), pick sensible defaults
   and state them — do not stall asking questions.
2. Draw with `command_line` (AutoCAD syntax, e.g. `rectang 0,0 100,60`) or
   `execute` with JSON for exact parameters.
3. Verify with `inspect_drawing {}` and `query_entities`, then `render {}` and
   describe what was drawn, including the real extents.
