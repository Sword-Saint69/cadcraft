# CADCraft plugin for Claude Code

One-install setup that wires the CADCraft MCP server, a drafting skill, and
two slash commands (`/cadcraft:status`, `/cadcraft:draft`) into Claude Code.

## Install

From the CADCraft checkout:

```sh
claude plugin marketplace add ./
claude plugin install cadcraft@cadcraft-marketplace
```

Or straight from GitHub:

```sh
claude plugin marketplace add storytold/cadcraft
claude plugin install cadcraft@cadcraft-marketplace
```

For development without installing:

```sh
claude --plugin-dir plugins/cadcraft
```

Validate the packaging (needs Claude Code on PATH):

```sh
claude plugin validate ./plugins/cadcraft
```

## How the server starts

The plugin launches `scripts/cadcraft-mcp.sh`, which finds `cadcraft-cli` in
this order: `$CADCRAFT_CLI`, `cadcraft-cli` on `PATH`, the checkout's
`target/release` / `target/debug` build, then `cargo run -p cadcraft-cli`.
Install the binary once and the plugin just works:

```sh
cargo install --path apps/cadcraft-cli
```

## Headless vs bridged

- Default: headless in-process session. Nothing else to run.
- Bridged (watch Claude draw in the live window): start the app with
  `cadcraft --control 7979`, then set the plugin's `control_port` option to
  `7979` (`/config`, or `pluginConfigs` in settings). The script passes
  `--connect 127.0.0.1:7979` and the UI tools (`screenshot`, `ui_inspect`,
  `ui_menu_invoke`, …) appear.

## Contents

- `.claude-plugin/plugin.json` — manifest: MCP server + `control_port` option.
- `scripts/cadcraft-mcp.sh` — server launcher (no dependencies beyond sh).
- `skills/drafting/SKILL.md` — drafting workflow loaded into context.
- `commands/status.md`, `commands/draft.md` — `/cadcraft:status`, `/cadcraft:draft`.
- `../../.claude-plugin/marketplace.json` — the marketplace entry.
