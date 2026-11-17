// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer Abdullah

//! The MCP server on its own, for development and the end-to-end test.
//! What ships is the same server inside the app: `rusteditor --mcp`.
//!
//! usage: rusteditor-mcp [auto|window|headless]   (or RUSTEDITOR_MCP_MODE)

use std::process::ExitCode;

fn main() -> ExitCode {
    let word = std::env::args()
        .nth(1)
        .or_else(|| std::env::var("RUSTEDITOR_MCP_MODE").ok())
        .unwrap_or_default();
    let Some(mode) = rusteditor_mcp::Mode::parse(&word) else {
        eprintln!("usage: rusteditor-mcp [auto|window|headless]");
        return ExitCode::from(2);
    };
    match rusteditor_mcp::run(mode) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("rusteditor mcp: {error}");
            ExitCode::FAILURE
        }
    }
}
