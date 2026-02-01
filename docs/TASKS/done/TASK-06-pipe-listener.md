# TASK-06: Pipe Listener (Unix Socket / Named Pipe)

## Cel

Cross-platform pipe listener — child nasłuchuje, parent się łączy.

## Plik

`src/transport/pipe.rs`

## Zakres

- [ ] Generowanie ścieżki pipe (platform-specific)
- [ ] Implementacja listener (Unix Socket na Linux/macOS)
- [ ] Implementacja listener (Named Pipe na Windows)
- [ ] Cleanup socket file po zamknięciu (Unix only)
- [ ] Testy jednostkowe

## Implementacja

### Generowanie ścieżki

```rust
use std::process;
use rand::Rng;

pub fn generate_pipe_path() -> String {
    let pid = process::id();
    let rand: String = rand::thread_rng()
        .sample_iter(&rand::distributions::Alphanumeric)
        .take(8)
        .map(char::from)
        .collect();

    if cfg!(windows) {
        format!(r"\\.\pipe\procwire-{}-{}", pid, rand)
    } else {
        format!("/tmp/procwire-{}-{}.sock", pid, rand)
    }
}
```

### Unix Listener

```rust
#[cfg(unix)]
pub async fn create_pipe_listener(path: &str) -> Result<tokio::net::UnixListener, ProcwireError> {
    // Usuń stary socket jeśli istnieje
    let _ = std::fs::remove_file(path);

    tokio::net::UnixListener::bind(path)
        .map_err(|e| ProcwireError::Io(e))
}

#[cfg(unix)]
pub async fn accept_connection(listener: tokio::net::UnixListener) -> Result<tokio::net::UnixStream, ProcwireError> {
    let (stream, _addr) = listener.accept().await?;
    Ok(stream)
}
```

### Windows Listener

```rust
#[cfg(windows)]
pub async fn create_pipe_listener(path: &str) -> Result<tokio::net::windows::named_pipe::NamedPipeServer, ProcwireError> {
    use tokio::net::windows::named_pipe::ServerOptions;

    ServerOptions::new()
        .first_pipe_instance(true)
        .create(path)
        .map_err(|e| ProcwireError::Io(e))
}

#[cfg(windows)]
pub async fn accept_connection(server: tokio::net::windows::named_pipe::NamedPipeServer) -> Result<tokio::net::windows::named_pipe::NamedPipeServer, ProcwireError> {
    server.connect().await?;
    Ok(server)
}
```

### Cleanup

```rust
pub struct PipeCleanup {
    path: String,
}

impl Drop for PipeCleanup {
    fn drop(&mut self) {
        #[cfg(unix)]
        let _ = std::fs::remove_file(&self.path);
        // Windows Named Pipes są automatycznie usuwane
    }
}
```

## Testy

- [ ] Listener bind + accept (loopback test z mock client)
- [ ] Poprawna ścieżka na danej platformie
- [ ] Cleanup after close (Unix)
- [ ] Path generation randomness

## Definition of Done

- [ ] `cargo test transport::pipe` — PASS na Linux/macOS
- [ ] Windows: testy jeśli CI ma Windows runner
- [ ] Cleanup działa poprawnie

## Kontekst

- Child tworzy pipe listener PRZED wysłaniem $init
- Parent łączy się do pipe PO otrzymaniu $init
- Tylko jedno połączenie jest akceptowane (1:1 parent-child)

## Uwagi platformowe

| Platforma | Typ | Format ścieżki |
|-----------|-----|----------------|
| Linux | Unix Domain Socket | `/tmp/procwire-{pid}-{rand}.sock` |
| macOS | Unix Domain Socket | `/tmp/procwire-{pid}-{rand}.sock` |
| Windows | Named Pipe | `\\.\pipe\procwire-{pid}-{rand}` |
