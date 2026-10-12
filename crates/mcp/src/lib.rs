//! CADCraft MCP server.
//!
//! JSON-RPC 2.0 over stdio (newline-delimited), implementing the MCP lifecycle, tools and
//! resources. Tools map onto the app's control-channel methods, served either by a headless
//! [`Headless`] session in-process or by a running app through its loopback control port
//! ([`Remote`]). Agents can type at the command line exactly like a user (`command_line`), call
//! any command with JSON (`execute`), inspect the drawing and render it to a PNG. Poll
//! `cmdline_state`, confirm with `press_enter`, cancel with `cancel`. When bridged to the app,
//! UI tools (`ui_inspect`, `ui_elements`, `ui_menu_list`/`ui_menu_invoke`, `ui_click`,
//! `ui_click_screen`, `ui_move`, `ui_drag`, `ui_key`, `ui_text`, `ui_set`, `ui_resize`,
//! `ui_focus`, `screenshot`, `quit`) drive the live window.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

mod backend;
mod server;
mod tools;

pub use backend::{Backend, Headless, Remote};
pub use server::{PROTOCOL_VERSION, Server};

/// The MCP tool catalog (connected `has_ui=true` lists UI tools too). Used by
/// `cadcraft-cli mcp --help` so docs and binary never drift.
pub fn tool_definitions_json(has_ui: bool) -> serde_json::Value {
    tools::tool_definitions(has_ui)
}

#[cfg(test)]
mod tests;
