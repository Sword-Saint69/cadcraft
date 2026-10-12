---
description: Inspect the active CADCraft drawing and summarize it.
---

Inspect the active CADCraft drawing with `inspect_drawing {}` and summarize:
entity counts, extents, layers, blocks, layouts, selection, and undo history.
If anything looks off, pull the relevant entities with `query_entities` and
finish with a `render {}` so the user can see the current state.
