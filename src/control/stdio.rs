//! Stdio I/O for control plane.
//!
//! The control plane uses stdio (stdin/stdout) for JSON-RPC communication.
//! Currently only `$init` message is implemented.
//!
//! # Important
//!
//! - **stdout**: JSON-RPC messages (one per line)
//! - **stderr**: Logs, debug output (not parsed by parent)
//! - **Never use `println!`**: It may add `\r\n` on Windows
//!
//! # Example
//!
//! ```ignore
//! use procwire_client::control::{write_stdout_line, build_init_message, InitSchema};
//!
//! let schema = InitSchema::new();
//! let msg = build_init_message("/tmp/pipe.sock", &schema);
//! write_stdout_line(&msg)?;
//! ```

use std::io::Write;

/// Write a line to stdout (Control Plane).
///
/// Writes the string followed by a single `\n` and flushes.
///
/// # Important
///
/// - Uses explicit `\n`, NOT `println!` (which may add `\r\n` on Windows)
/// - Flushes immediately (parent waits for complete line)
/// - Any logging should go to stderr, not stdout
///
/// # Errors
///
/// Returns IO error if write or flush fails.
pub fn write_stdout_line(line: &str) -> std::io::Result<()> {
    let stdout = std::io::stdout();
    let mut handle = stdout.lock();
    handle.write_all(line.as_bytes())?;
    handle.write_all(b"\n")?;
    handle.flush()?;
    Ok(())
}

/// Write a JSON value to stdout as a single line.
///
/// Serializes the value to JSON and writes it to stdout.
///
/// # Errors
///
/// Returns error if serialization or write fails.
pub fn write_stdout_json<T: serde::Serialize>(value: &T) -> crate::error::Result<()> {
    let json = serde_json::to_string(value)?;
    write_stdout_line(&json)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_write_stdout_line_does_not_panic() {
        // We can't easily capture stdout in tests, but we can verify
        // the function doesn't panic and returns Ok
        let result = write_stdout_line(r#"{"test": true}"#);
        assert!(result.is_ok());
    }

    #[test]
    fn test_write_stdout_json_serializes() {
        use serde::Serialize;

        #[derive(Serialize)]
        struct TestData {
            value: i32,
        }

        let data = TestData { value: 42 };
        let result = write_stdout_json(&data);
        assert!(result.is_ok());
    }
}
