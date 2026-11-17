// SPDX-License-Identifier: AGPL-3.0-or-later

//! The MCP relay: what makes the link from the MCP tab work.
//!
//! An AI talks MCP to `https://editormcp.wisp.uno/<code>`. That server
//! edits nothing: it queues each message, and this module, running while
//! the app is open, fetches them (`GET <link>/api/poll`), answers each one
//! through the same MCP server that `rusteditor --mcp` is, standing on the
//! window's own API, and posts the answer back (`POST <link>/api/reply`).
//! So the AI has exactly the tools this build has, and its edits land in
//! the project that is open. The window's API must be on (Settings >
//! Automation); until it is, the relay says so and tries again.

use std::io::{BufRead, BufReader, Write};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use rusteditor_host::net::{self, Trust};
use rusteditor_mcp::{Backend, Window, protocol};
use serde_json::Value;

/// How long the server may hold one poll open: asked for, not trusted;
/// the read timeout below is longer.
const WAIT_SECS: u64 = 25;
const CONNECT: Duration = Duration::from_secs(10);
const READ: Duration = Duration::from_secs(45);

/// A running relay. Dropping it stops it; it ends within one poll.
pub struct Relay {
    stop: Arc<AtomicBool>,
}

impl Drop for Relay {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// Starts relaying for `link` (`https://host/<code>`) on a thread of its
/// own. `status` is told, in words, what is happening: it is called from
/// that thread.
pub fn start(link: String, status: impl Fn(String) + Send + 'static) -> Relay {
    let stop = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&stop);
    let spawned = std::thread::Builder::new()
        .name("rusteditor mcp relay".to_owned())
        .spawn(move || run(&link, &flag, &status));
    if spawned.is_err() {
        log::warn!("mcp relay: could not start its thread");
    }
    Relay { stop }
}

fn run(link: &str, stop: &AtomicBool, status: &dyn Fn(String)) {
    while !stop.load(Ordering::Relaxed) {
        match Window::find() {
            Ok(window) => session(link, stop, Box::new(window), status),
            Err(why) => {
                status(format!("Waiting for Automation: {why}"));
                nap(stop, Duration::from_secs(3));
            }
        }
    }
}

/// One stretch with the window's API reachable: poll, hand each message to
/// the MCP server, post each of its answers. Returns when stopped.
fn session(link: &str, stop: &AtomicBool, backend: Box<dyn Backend>, status: &dyn Fn(String)) {
    let (input, mut to_server) = match std::io::pipe() {
        Ok(pipe) => pipe,
        Err(error) => return status(format!("Could not start the relay: {error}")),
    };
    let (answers, from_server) = match std::io::pipe() {
        Ok(pipe) => pipe,
        Err(error) => return status(format!("Could not start the relay: {error}")),
    };
    let serving = std::thread::Builder::new()
        .name("rusteditor mcp serve".to_owned())
        .spawn(move || protocol::serve(BufReader::new(input), from_server, backend));
    let reply_link = link.to_owned();
    let replying = std::thread::Builder::new()
        .name("rusteditor mcp reply".to_owned())
        .spawn(move || {
            for line in BufReader::new(answers).lines() {
                let Ok(line) = line else { break };
                if is_answer(&line) {
                    if let Err(why) = send_answer(&reply_link, &line) {
                        log::warn!("mcp relay: an answer could not be posted: {why}");
                    }
                }
            }
        });
    if serving.is_err() || replying.is_err() {
        return status("Could not start the relay.".to_owned());
    }

    let mut failures = 0u64;
    let mut connected = false;
    'polling: while !stop.load(Ordering::Relaxed) {
        match poll(link) {
            Ok(items) => {
                failures = 0;
                if !connected {
                    connected = true;
                    status("Connected. Give the link to your AI.".to_owned());
                }
                for item in items {
                    if writeln!(to_server, "{item}")
                        .and_then(|()| to_server.flush())
                        .is_err()
                    {
                        break 'polling;
                    }
                }
            }
            Err(why) => {
                failures += 1;
                connected = false;
                status(format!("Cannot reach the MCP server: {why}"));
                nap(stop, Duration::from_secs((failures * 2).min(10)));
            }
        }
    }
    // The server sees the end of its input and finishes; the reply thread
    // sees the end of the server's output.
    drop(to_server);
    for handle in [serving, replying].into_iter().flatten() {
        let _ = handle.join();
    }
}

/// The messages the server was holding for the app, waiting a while for
/// one when it has none.
fn poll(link: &str) -> Result<Vec<Value>, String> {
    let url = format!("{link}/api/poll?wait={WAIT_SECS}");
    let response = net::get(&url, CONNECT, READ, Trust::System, &|request| request)
        .map_err(|failure| failure.to_string())?;
    let body = response.into_string().map_err(|error| error.to_string())?;
    items_from(&body)
}

fn items_from(body: &str) -> Result<Vec<Value>, String> {
    let value: Value = serde_json::from_str(body).map_err(|error| error.to_string())?;
    match value.get("items") {
        Some(Value::Array(items)) => Ok(items.clone()),
        _ => Err("the server's answer has no items".to_owned()),
    }
}

/// A line the MCP server wrote that answers a request, as opposed to the
/// progress notifications it also writes, which an AI over this link
/// cannot receive.
fn is_answer(line: &str) -> bool {
    serde_json::from_str::<Value>(line)
        .map(|value| {
            value.get("id").is_some()
                && (value.get("result").is_some() || value.get("error").is_some())
        })
        .unwrap_or(false)
}

fn send_answer(link: &str, line: &str) -> Result<(), String> {
    net::post(
        &format!("{link}/api/reply"),
        CONNECT,
        CONNECT,
        Trust::System,
        line,
    )
    .map(|_| ())
    .map_err(|failure| failure.to_string())
}

/// Sleeps `total`, a second at a time, ending early when stopped.
fn nap(stop: &AtomicBool, total: Duration) {
    let mut left = total;
    while !left.is_zero() && !stop.load(Ordering::Relaxed) {
        let step = left.min(Duration::from_secs(1));
        std::thread::sleep(step);
        left -= step;
    }
}

#[cfg(test)]
mod tests {
    use super::{is_answer, items_from};

    #[test]
    fn only_answers_are_posted() {
        assert!(is_answer(r#"{"jsonrpc":"2.0","id":3,"result":{}}"#));
        assert!(is_answer(
            r#"{"jsonrpc":"2.0","id":3,"error":{"code":1,"message":"x"}}"#
        ));
        assert!(!is_answer(
            r#"{"jsonrpc":"2.0","method":"notifications/progress","params":{}}"#
        ));
        assert!(!is_answer("not json"));
    }

    #[test]
    fn a_poll_answer_is_its_items() {
        assert_eq!(items_from(r#"{"items":[]}"#), Ok(vec![]));
        assert_eq!(
            items_from(r#"{"items":[{"id":1}]}"#).map(|items| items.len()),
            Ok(1)
        );
        assert!(items_from(r#"{"nope":1}"#).is_err());
        assert!(items_from("<html>").is_err());
    }
}
