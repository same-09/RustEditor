// SPDX-License-Identifier: AGPL-3.0-or-later

//! The MCP link (the library's MCP page): one press of Generate MCP asks
//! the RustEditor MCP server for a new workspace and gets back a random
//! code; the link an AI is given is `https://editormcp.wisp.uno/<code>`.

use std::time::Duration;

use crate::net::{self, Trust};

/// The server that hands out the codes and speaks MCP at `BASE/<code>`.
pub const BASE: &str = "https://editormcp.wisp.uno";

/// Asks the server for a new code and returns the whole link. The error is
/// a sentence a person can read.
pub fn generate() -> Result<String, String> {
    let url = format!("{BASE}/api/new");
    let response = net::post(
        &url,
        Duration::from_secs(10),
        Duration::from_secs(20),
        Trust::System,
        "",
    )
    .map_err(|failure| match failure.error {
        ureq::Error::Status(429, _) => "The server is full. Try again later.".to_string(),
        _ => format!("Could not reach the MCP server: {failure}"),
    })?;
    let body = response
        .into_string()
        .map_err(|error| format!("The MCP server's answer could not be read: {error}"))?;
    link_from(&body).ok_or_else(|| "The MCP server gave an answer that is not a code.".to_string())
}

/// The link for the answer `{"code":"<20 hex digits>"}`; none for anything
/// else, so a page that is not the server never becomes a link.
fn link_from(body: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(body).ok()?;
    let code = value.get("code")?.as_str()?;
    let ok = code.len() == 20
        && code
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
    ok.then(|| format!("{BASE}/{code}"))
}

#[cfg(test)]
mod tests {
    use super::link_from;

    #[test]
    fn a_code_becomes_a_link() {
        assert_eq!(
            link_from(r#"{"code":"0123456789abcdef0123"}"#).as_deref(),
            Some("https://editormcp.wisp.uno/0123456789abcdef0123")
        );
    }

    #[test]
    fn anything_else_is_no_link() {
        assert_eq!(link_from("<html>"), None);
        assert_eq!(link_from(r#"{"code":"short"}"#), None);
        assert_eq!(link_from(r#"{"code":"../../etc/passwd/zzzz"}"#), None);
        assert_eq!(link_from(r#"{"other":1}"#), None);
    }
}
