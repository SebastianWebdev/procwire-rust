//! `$init` message builder.
//!
//! The `$init` message is sent via stdout to tell the parent:
//! - The pipe path for data plane connection
//! - The schema (methods and events with their IDs)
//! - Protocol version
//!
//! # Example
//!
//! ```
//! use procwire_client::control::{InitSchema, ResponseType, build_init_message};
//!
//! let mut schema = InitSchema::new();
//! schema.add_method("echo", 1, ResponseType::Result);
//! schema.add_method("generate", 2, ResponseType::Stream);
//! schema.add_event("progress", 3);
//!
//! let json = build_init_message("/tmp/procwire.sock", &schema);
//! assert!(json.contains("$init"));
//! ```

use serde::Serialize;
use serde_json::json;
use std::collections::HashMap;

/// Response type for a method.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ResponseType {
    /// Returns a single result.
    Result,
    /// Returns a stream of chunks.
    Stream,
    /// Returns only an acknowledgment.
    Ack,
    /// Fire-and-forget, no response.
    None,
}

/// Method definition with ID and response type.
#[derive(Debug, Clone)]
pub struct MethodSchema {
    /// Assigned method ID (1-65534).
    pub id: u16,
    /// Expected response type.
    pub response: ResponseType,
}

/// Event definition with ID.
#[derive(Debug, Clone)]
pub struct EventSchema {
    /// Assigned event ID (1-65534).
    pub id: u16,
}

/// Schema describing available methods and events.
///
/// Used to build the `$init` message that tells the parent
/// what methods and events this worker supports.
#[derive(Debug, Clone, Default)]
pub struct InitSchema {
    /// Map of method names to their definitions.
    pub methods: HashMap<String, MethodSchema>,
    /// Map of event names to their definitions.
    pub events: HashMap<String, EventSchema>,
}

impl InitSchema {
    /// Create a new empty schema.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a method to the schema.
    ///
    /// # Arguments
    ///
    /// * `name` - Method name (e.g., "echo")
    /// * `id` - Assigned method ID (must be 1-65534)
    /// * `response` - Expected response type
    pub fn add_method(&mut self, name: &str, id: u16, response: ResponseType) {
        self.methods
            .insert(name.to_string(), MethodSchema { id, response });
    }

    /// Add an event to the schema.
    ///
    /// # Arguments
    ///
    /// * `name` - Event name (e.g., "progress")
    /// * `id` - Assigned event ID (must be 1-65534)
    pub fn add_event(&mut self, name: &str, id: u16) {
        self.events.insert(name.to_string(), EventSchema { id });
    }

    /// Get a method by name.
    pub fn get_method(&self, name: &str) -> Option<&MethodSchema> {
        self.methods.get(name)
    }

    /// Get an event by name.
    pub fn get_event(&self, name: &str) -> Option<&EventSchema> {
        self.events.get(name)
    }

    /// Check if schema is empty.
    pub fn is_empty(&self) -> bool {
        self.methods.is_empty() && self.events.is_empty()
    }
}

/// Protocol version string.
pub const PROTOCOL_VERSION: &str = "2.0.0";

/// Build the `$init` JSON-RPC message.
///
/// This creates the JSON message that should be sent to the parent
/// via stdout to initialize the connection.
///
/// # Arguments
///
/// * `pipe_path` - Path to the pipe/socket for data plane
/// * `schema` - Schema describing methods and events
///
/// # Returns
///
/// JSON string ready to be written to stdout.
pub fn build_init_message(pipe_path: &str, schema: &InitSchema) -> String {
    let methods: serde_json::Map<String, serde_json::Value> = schema
        .methods
        .iter()
        .map(|(name, m)| {
            (
                name.clone(),
                json!({
                    "id": m.id,
                    "response": m.response
                }),
            )
        })
        .collect();

    let events: serde_json::Map<String, serde_json::Value> = schema
        .events
        .iter()
        .map(|(name, e)| (name.clone(), json!({ "id": e.id })))
        .collect();

    let msg = json!({
        "jsonrpc": "2.0",
        "method": "$init",
        "params": {
            "pipe": pipe_path,
            "schema": {
                "methods": methods,
                "events": events
            },
            "version": PROTOCOL_VERSION
        }
    });

    serde_json::to_string(&msg).expect("JSON serialization should not fail")
}

// Legacy types for backward compatibility
// TODO: Remove after migration

/// The `$init` message (legacy struct, prefer `build_init_message`).
#[derive(Debug, Serialize)]
pub struct InitMessage {
    jsonrpc: &'static str,
    method: &'static str,
    params: InitParams,
}

/// Parameters for the `$init` message (legacy).
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InitParams {
    /// Path to the named pipe or Unix socket.
    pub pipe_path: String,
    /// Schema describing available methods and events.
    pub schema: Schema,
}

/// Schema (legacy, prefer `InitSchema`).
#[derive(Debug, Default, Serialize)]
pub struct Schema {
    /// Map of method names to their definitions.
    pub methods: HashMap<String, MethodDef>,
    /// Map of event names to their definitions.
    pub events: HashMap<String, EventDef>,
}

/// Method definition (legacy).
#[derive(Debug, Serialize)]
pub struct MethodDef {
    /// Assigned method ID.
    pub id: u16,
}

/// Event definition (legacy).
#[derive(Debug, Serialize)]
pub struct EventDef {
    /// Assigned event ID.
    pub id: u16,
}

impl InitMessage {
    /// Create a new `$init` message (legacy).
    pub fn new(pipe_path: String, schema: Schema) -> Self {
        Self {
            jsonrpc: "2.0",
            method: "$init",
            params: InitParams { pipe_path, schema },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_init_message_format() {
        let mut schema = InitSchema::new();
        schema.add_method("echo", 1, ResponseType::Result);
        schema.add_method("generate", 2, ResponseType::Stream);
        schema.add_event("progress", 3);

        let msg = build_init_message("/tmp/test.sock", &schema);
        let parsed: serde_json::Value = serde_json::from_str(&msg).unwrap();

        assert_eq!(parsed["jsonrpc"], "2.0");
        assert_eq!(parsed["method"], "$init");
        assert_eq!(parsed["params"]["pipe"], "/tmp/test.sock");
        assert_eq!(parsed["params"]["version"], "2.0.0");
    }

    #[test]
    fn test_method_schema() {
        let mut schema = InitSchema::new();
        schema.add_method("echo", 1, ResponseType::Result);

        let msg = build_init_message("/tmp/test.sock", &schema);
        let parsed: serde_json::Value = serde_json::from_str(&msg).unwrap();

        assert_eq!(parsed["params"]["schema"]["methods"]["echo"]["id"], 1);
        assert_eq!(
            parsed["params"]["schema"]["methods"]["echo"]["response"],
            "result"
        );
    }

    #[test]
    fn test_event_schema() {
        let mut schema = InitSchema::new();
        schema.add_event("progress", 5);

        let msg = build_init_message("/tmp/test.sock", &schema);
        let parsed: serde_json::Value = serde_json::from_str(&msg).unwrap();

        assert_eq!(parsed["params"]["schema"]["events"]["progress"]["id"], 5);
    }

    #[test]
    fn test_response_types() {
        let mut schema = InitSchema::new();
        schema.add_method("result_method", 1, ResponseType::Result);
        schema.add_method("stream_method", 2, ResponseType::Stream);
        schema.add_method("ack_method", 3, ResponseType::Ack);
        schema.add_method("none_method", 4, ResponseType::None);

        let msg = build_init_message("/tmp/test.sock", &schema);
        let parsed: serde_json::Value = serde_json::from_str(&msg).unwrap();

        let methods = &parsed["params"]["schema"]["methods"];
        assert_eq!(methods["result_method"]["response"], "result");
        assert_eq!(methods["stream_method"]["response"], "stream");
        assert_eq!(methods["ack_method"]["response"], "ack");
        assert_eq!(methods["none_method"]["response"], "none");
    }

    #[test]
    fn test_empty_schema() {
        let schema = InitSchema::new();
        assert!(schema.is_empty());

        let msg = build_init_message("/tmp/test.sock", &schema);
        let parsed: serde_json::Value = serde_json::from_str(&msg).unwrap();

        assert!(parsed["params"]["schema"]["methods"]
            .as_object()
            .unwrap()
            .is_empty());
        assert!(parsed["params"]["schema"]["events"]
            .as_object()
            .unwrap()
            .is_empty());
    }

    #[test]
    fn test_schema_get_method() {
        let mut schema = InitSchema::new();
        schema.add_method("echo", 1, ResponseType::Result);

        let method = schema.get_method("echo").unwrap();
        assert_eq!(method.id, 1);
        assert_eq!(method.response, ResponseType::Result);

        assert!(schema.get_method("nonexistent").is_none());
    }

    #[test]
    fn test_schema_get_event() {
        let mut schema = InitSchema::new();
        schema.add_event("progress", 5);

        let event = schema.get_event("progress").unwrap();
        assert_eq!(event.id, 5);

        assert!(schema.get_event("nonexistent").is_none());
    }
}
