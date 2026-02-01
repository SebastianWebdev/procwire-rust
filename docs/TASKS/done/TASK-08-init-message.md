# TASK-08: $init Message Builder

## Cel

Budowanie i wysyłanie wiadomości `$init` na stdout.

## Plik

`src/control/init.rs`

## Zakres

- [ ] Definicja typów schematu (MethodSchema, EventSchema, ResponseType)
- [ ] Implementacja `InitSchema` struct
- [ ] Implementacja `build_init_message()`
- [ ] Testy jednostkowe

## Implementacja

### Typy schematu

```rust
use serde::Serialize;
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ResponseType {
    Result,
    Stream,
    Ack,
    None,
}

#[derive(Debug, Clone)]
pub struct MethodSchema {
    pub id: u16,
    pub response: ResponseType,
}

#[derive(Debug, Clone)]
pub struct EventSchema {
    pub id: u16,
}

#[derive(Debug, Clone, Default)]
pub struct InitSchema {
    pub methods: HashMap<String, MethodSchema>,
    pub events: HashMap<String, EventSchema>,
}
```

### Builder

```rust
impl InitSchema {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_method(&mut self, name: &str, id: u16, response: ResponseType) {
        self.methods.insert(name.to_string(), MethodSchema { id, response });
    }

    pub fn add_event(&mut self, name: &str, id: u16) {
        self.events.insert(name.to_string(), EventSchema { id });
    }
}
```

### Build init message

```rust
use serde_json::json;

pub fn build_init_message(pipe_path: &str, schema: &InitSchema) -> String {
    let methods: serde_json::Map<String, serde_json::Value> = schema.methods
        .iter()
        .map(|(name, m)| {
            (name.clone(), json!({
                "id": m.id,
                "response": m.response
            }))
        })
        .collect();

    let events: serde_json::Map<String, serde_json::Value> = schema.events
        .iter()
        .map(|(name, e)| {
            (name.clone(), json!({ "id": e.id }))
        })
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
            "version": "2.0.0"
        }
    });

    serde_json::to_string(&msg).expect("JSON serialization should not fail")
}
```

## Testy

- [ ] Poprawny JSON format
- [ ] Poprawne ID assignment
- [ ] Poprawne response type strings ("result", "stream", "ack", "none")
- [ ] Version field present ("2.0.0")
- [ ] Pipe path w params

### Test

```rust
#[test]
fn test_init_message_format() {
    let mut schema = InitSchema::new();
    schema.add_method("echo", 1, ResponseType::Result);
    schema.add_method("generate", 2, ResponseType::Stream);
    schema.add_event("progress", 1);

    let msg = build_init_message("/tmp/test.sock", &schema);
    let parsed: serde_json::Value = serde_json::from_str(&msg).unwrap();

    assert_eq!(parsed["jsonrpc"], "2.0");
    assert_eq!(parsed["method"], "$init");
    assert_eq!(parsed["params"]["pipe"], "/tmp/test.sock");
    assert_eq!(parsed["params"]["version"], "2.0.0");
    assert_eq!(parsed["params"]["schema"]["methods"]["echo"]["id"], 1);
    assert_eq!(parsed["params"]["schema"]["methods"]["echo"]["response"], "result");
}
```

## Definition of Done

- [ ] `cargo test control::init` — PASS
- [ ] JSON output zgodny z formatem oczekiwanym przez Node.js parent
- [ ] Doc comments

## Kontekst

- `$init` jest JEDYNĄ wiadomością JSON wysyłaną przez child
- Parent używa `$init` do:
  1. Poznania ścieżki pipe
  2. Walidacji schematu (czy child ma wymagane metody)
  3. Budowania lookup maps (nazwa → ID)
- Method IDs są przydzielane przez child (kolejno od 1)
