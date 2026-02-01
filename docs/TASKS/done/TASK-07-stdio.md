# TASK-07: Stdio I/O (Control Plane)

## Cel

Wysyłanie JSON-RPC przez stdout (Control Plane).

## Plik

`src/control/stdio.rs`

## Zakres

- [ ] Implementacja `write_stdout_line()` dla JSON-RPC
- [ ] Upewnienie się że używamy explicit `\n` (nie println!)
- [ ] Przygotowanie pod przyszły async stdin reader
- [ ] Testy jednostkowe

## Implementacja

### Write to stdout

```rust
use std::io::Write;

/// Wysyła linię JSON na stdout (Control Plane).
///
/// WAŻNE: NIE używaj `println!` — parent parsuje stdout line-by-line
/// i `println!` dodaje system-dependent line endings.
/// Używamy explicit `\n`.
pub fn write_stdout_line(json: &str) -> std::io::Result<()> {
    let stdout = std::io::stdout();
    let mut handle = stdout.lock();
    handle.write_all(json.as_bytes())?;
    handle.write_all(b"\n")?;
    handle.flush()?;
    Ok(())
}
```

### Przyszły stdin reader (placeholder)

```rust
// TODO: Implementacja w przyszłości dla heartbeat/shutdown
// pub async fn read_stdin_lines() -> impl Stream<Item = String> { ... }
```

## Testy

- [ ] `write_stdout_line` wysyła poprawną linię
- [ ] Kończy się dokładnie jednym `\n`
- [ ] Nie dodaje żadnych dodatkowych znaków

### Test (wymaga capture stdout)

```rust
#[test]
fn test_write_stdout_line_format() {
    // Test przez sprawdzenie że funkcja nie panikuje
    // i zwraca Ok
    let result = write_stdout_line(r#"{"jsonrpc":"2.0","method":"$init"}"#);
    assert!(result.is_ok());
}
```

## Definition of Done

- [ ] Funkcja `write_stdout_line` działa poprawnie
- [ ] Żadne logi/debug output nie idą na stdout (muszą iść na stderr)
- [ ] Doc comments

## Kontekst

- Control Plane używa stdio (stdin/stdout) dla JSON-RPC
- Jedyna wiadomość implementowana w v2.0 to `$init`
- stdout jest parsowany line-by-line przez parent
- stderr jest przekierowany na `inherit` (logi child trafiają do terminala)

## Uwagi

1. **NIGDY nie używaj `println!` na stdout** — może dodać `\r\n` na Windows
2. **Logi (`tracing`) idą na stderr** — nie na stdout
3. **Flush jest obowiązkowy** — parent czeka na pełną linię
