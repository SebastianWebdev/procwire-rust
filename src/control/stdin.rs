//! Control-plane stdin reader (`$ping`/`$pong` heartbeat and `$shutdown`).
//!
//! The parent may send newline-delimited JSON-RPC 2.0 messages to the child's
//! **stdin**. The Rust client must react to two of them to stay compatible with
//! a Procwire v2 parent:
//!
//! - `$ping`  → reply with `$pong` on **stdout** (liveness heartbeat). A parent
//!   configured with `spawnPolicy({ heartbeat: ... })` kills any child that does
//!   not answer in time, so this is **required**.
//! - `$shutdown` → close down cleanly and let the process exit promptly instead
//!   of being force-killed after the parent's 5 s grace period.
//!
//! Per the protocol, any line that does **not** start with `{` is ignored, and
//! unknown methods are ignored too.
//!
//! # Threading
//!
//! [`run_control_reader`] performs blocking reads on stdin and is meant to run
//! on a dedicated OS thread (see [`crate::client::Client`]). Running it on a
//! plain `std` thread — rather than the Tokio blocking pool — guarantees it can
//! never keep the async runtime (or the process) alive: once `main` returns the
//! abandoned blocking read is simply terminated with the process.

use tokio_util::sync::CancellationToken;

use super::stdio::write_stdout_line;

/// The `$pong` reply written to stdout in response to a `$ping`.
///
/// A trailing newline is added by [`write_stdout_line`].
pub const PONG_MESSAGE: &str = r#"{"jsonrpc":"2.0","method":"$pong"}"#;

/// Action to take for a single control-plane line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ControlAction {
    /// `$ping` received — reply with `$pong`.
    Pong,
    /// `$shutdown` received — shut down gracefully.
    Shutdown,
    /// Non-JSON line, unknown method, or anything else — ignore it.
    Ignore,
}

/// Parse a single control-plane line into a [`ControlAction`].
///
/// Lines that do not start with `{` (after trimming) are ignored, matching the
/// reference reader which "ignores any line that does not start with `{`".
pub(crate) fn parse_control_line(line: &str) -> ControlAction {
    let trimmed = line.trim();

    if !trimmed.starts_with('{') {
        return ControlAction::Ignore;
    }

    let value: serde_json::Value = match serde_json::from_str(trimmed) {
        Ok(value) => value,
        Err(_) => return ControlAction::Ignore,
    };

    match value.get("method").and_then(serde_json::Value::as_str) {
        Some("$ping") => ControlAction::Pong,
        Some("$shutdown") => ControlAction::Shutdown,
        _ => ControlAction::Ignore,
    }
}

/// Run the control-plane reader loop until stdin closes or `$shutdown` arrives.
///
/// Reads newline-delimited JSON-RPC from stdin and:
/// - replies to `$ping` with `$pong` on stdout (heartbeat reflex), and
/// - cancels `shutdown_token` on `$shutdown` so the client can stop cleanly.
///
/// This call **blocks** the current thread; spawn it on a dedicated OS thread.
pub fn run_control_reader(shutdown_token: CancellationToken) {
    use std::io::BufRead;

    let stdin = std::io::stdin();
    let mut handle = stdin.lock();
    let mut line = String::new();

    loop {
        line.clear();
        match handle.read_line(&mut line) {
            // EOF — the parent closed our stdin; nothing more will arrive.
            Ok(0) => break,
            Ok(_) => match parse_control_line(&line) {
                ControlAction::Pong => {
                    // Best-effort: if stdout is gone the next read will fail/EOF.
                    if let Err(e) = write_stdout_line(PONG_MESSAGE) {
                        tracing::warn!("Failed to write $pong: {}", e);
                        break;
                    }
                }
                ControlAction::Shutdown => {
                    tracing::info!("Received $shutdown on control plane");
                    shutdown_token.cancel();
                    break;
                }
                ControlAction::Ignore => {}
            },
            Err(e) => {
                tracing::warn!("Control-plane stdin read error: {}", e);
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ping_maps_to_pong() {
        let line = r#"{"jsonrpc":"2.0","method":"$ping"}"#;
        assert_eq!(parse_control_line(line), ControlAction::Pong);
    }

    #[test]
    fn test_shutdown_maps_to_shutdown() {
        let line = r#"{"jsonrpc":"2.0","method":"$shutdown","params":{}}"#;
        assert_eq!(parse_control_line(line), ControlAction::Shutdown);
    }

    #[test]
    fn test_trailing_newline_is_tolerated() {
        let line = "{\"jsonrpc\":\"2.0\",\"method\":\"$ping\"}\n";
        assert_eq!(parse_control_line(line), ControlAction::Pong);
    }

    #[test]
    fn test_non_json_line_is_ignored() {
        assert_eq!(
            parse_control_line("hello, this is a log line"),
            ControlAction::Ignore
        );
        assert_eq!(parse_control_line(""), ControlAction::Ignore);
        assert_eq!(parse_control_line("   "), ControlAction::Ignore);
    }

    #[test]
    fn test_unknown_method_is_ignored() {
        let line = r#"{"jsonrpc":"2.0","method":"$something-else"}"#;
        assert_eq!(parse_control_line(line), ControlAction::Ignore);
    }

    #[test]
    fn test_missing_method_is_ignored() {
        let line = r#"{"jsonrpc":"2.0","id":1}"#;
        assert_eq!(parse_control_line(line), ControlAction::Ignore);
    }

    #[test]
    fn test_malformed_json_is_ignored() {
        let line = r#"{"jsonrpc":"2.0","method":"#;
        assert_eq!(parse_control_line(line), ControlAction::Ignore);
    }

    #[test]
    fn test_pong_message_shape() {
        let parsed: serde_json::Value = serde_json::from_str(PONG_MESSAGE).unwrap();
        assert_eq!(parsed["jsonrpc"], "2.0");
        assert_eq!(parsed["method"], "$pong");
    }
}
