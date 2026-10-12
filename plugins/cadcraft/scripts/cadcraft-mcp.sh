#!/bin/sh
# Launch the CADCraft MCP server for the Claude Code plugin.
#
# Resolution order for the `cadcraft-cli` binary:
#   1. $CADCRAFT_CLI (explicit path)
#   2. `cadcraft-cli` on PATH (cargo install / release package)
#   3. checkouts: <repo>/target/{release,debug}/cadcraft-cli (plugin ships in-repo)
#   4. `cargo run -p cadcraft-cli` from the enclosing checkout (needs a Rust toolchain)
#
# Mode: bridged to a running app when $CADCRAFT_CONTROL_PORT is set
# (the plugin's `control_port` option feeds this env var), else headless.

set -eu

set -- mcp
if [ -n "${CADCRAFT_CONTROL_PORT:-}" ]; then
    set -- mcp --connect "127.0.0.1:${CADCRAFT_CONTROL_PORT}"
fi

if [ -n "${CADCRAFT_CLI:-}" ] && [ -x "${CADCRAFT_CLI}" ]; then
    exec "${CADCRAFT_CLI}" "$@"
fi

if command -v cadcraft-cli >/dev/null 2>&1; then
    exec cadcraft-cli "$@"
fi

# The plugin lives at <repo>/plugins/cadcraft, so the checkout root is three levels up.
ROOT="$(CDPATH= cd -- "$(dirname "$0")/../../.." && pwd)"
if [ -x "${ROOT}/target/release/cadcraft-cli" ]; then
    exec "${ROOT}/target/release/cadcraft-cli" "$@"
fi
if [ -x "${ROOT}/target/debug/cadcraft-cli" ]; then
    exec "${ROOT}/target/debug/cadcraft-cli" "$@"
fi
if command -v cargo >/dev/null 2>&1 && [ -f "${ROOT}/Cargo.toml" ]; then
    exec cargo run -q --manifest-path "${ROOT}/Cargo.toml" -p cadcraft-cli -- "$@"
fi

echo "cadcraft-mcp: no cadcraft-cli found." >&2
echo "Install it (cargo install --path apps/cadcraft-cli) or set CADCRAFT_CLI." >&2
exit 1
