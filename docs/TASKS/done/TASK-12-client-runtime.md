# TASK-12: Client Runtime Loop

## Cel

Główna pętla klienta: start → init → accept → read frames → dispatch.

## Plik

`src/client.rs` (kontynuacja TASK-11)

## Zakres

- [ ] Implementacja `Client::start()`
- [ ] Wysłanie $init na stdout
- [ ] Akceptacja połączenia od parent
- [ ] Read loop (frame parsing + dispatch)
- [ ] Obsługa ABORT
- [ ] Event emitting
- [ ] Graceful shutdown
- [ ] Testy jednostkowe

## Implementacja

### Client struct

```rust
use std::sync::Arc;
use tokio::sync::{oneshot, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::handler::HandlerRegistry;
use crate::protocol::{FrameBuffer, Frame, ABORT_METHOD_ID, flags};
use crate::transport::pipe::{generate_pipe_path, create_pipe_listener, accept_connection};
use crate::control::stdio::write_stdout_line;
use crate::control::init::build_init_message;
use crate::error::ProcwireError;

pub struct Client {
    registry: Arc<HandlerRegistry>,
    pipe_writer: Arc<Mutex<PipeWriter>>,
    shutdown_rx: oneshot::Receiver<()>,
}

type PipeWriter = Box<dyn AsyncWriteExt + Send + Unpin>;
```

### Client::start()

```rust
impl Client {
    pub(crate) async fn start(registry: HandlerRegistry) -> Result<Self, ProcwireError> {
        // 1. Generate pipe path
        let pipe_path = generate_pipe_path();

        // 2. Start pipe listener
        let listener = create_pipe_listener(&pipe_path).await?;

        // 3. Build schema from registry
        let schema = registry.build_schema();

        // 4. Send $init to parent (stdout)
        let init_msg = build_init_message(&pipe_path, &schema);
        write_stdout_line(&init_msg)?;

        // 5. Accept parent connection
        let stream = accept_connection(listener).await?;

        // 6. Split stream into reader + writer
        let (reader, writer) = tokio::io::split(stream);
        let writer: PipeWriter = Box::new(writer);
        let writer = Arc::new(Mutex::new(writer));

        // 7. Spawn read loop
        let (shutdown_tx, shutdown_rx) = oneshot::channel();
        let registry = Arc::new(registry);
        let writer_clone = writer.clone();
        let registry_clone = registry.clone();

        tokio::spawn(async move {
            Self::read_loop(reader, registry_clone, writer_clone).await;
            let _ = shutdown_tx.send(());
        });

        Ok(Client {
            registry,
            pipe_writer: writer,
            shutdown_rx
        })
    }

    async fn read_loop<R: AsyncReadExt + Unpin>(
        mut reader: R,
        registry: Arc<HandlerRegistry>,
        writer: Arc<Mutex<PipeWriter>>,
    ) {
        let mut frame_buffer = FrameBuffer::new();
        let mut buf = vec![0u8; 64 * 1024]; // 64KB read buffer

        loop {
            match reader.read(&mut buf).await {
                Ok(0) => break, // Connection closed
                Ok(n) => {
                    let frames = frame_buffer.push(&buf[..n]);
                    for frame in frames {
                        Self::dispatch_frame(frame, &registry, &writer).await;
                    }
                }
                Err(e) => {
                    tracing::error!("Pipe read error: {}", e);
                    break;
                }
            }
        }
    }
```

### Dispatch logic

```rust
    async fn dispatch_frame(
        frame: Frame,
        registry: &HandlerRegistry,
        writer: &Arc<Mutex<PipeWriter>>,
    ) {
        let header = &frame.header;

        // Abort handling
        if header.method_id == ABORT_METHOD_ID {
            registry.abort(header.request_id);
            return;
        }

        // Lookup method
        let method_name = match registry.get_method_name(header.method_id) {
            Some(name) => name.to_string(),
            None => {
                tracing::warn!("Unknown method ID: {}", header.method_id);
                // TODO: Send error response
                return;
            }
        };

        // Create context
        let ctx = RequestContext::new(
            header.request_id,
            method_name.clone(),
            header.method_id,
            writer.clone(),
        );

        // Get handler
        let handler = match registry.get_handler(&method_name) {
            Some(h) => h,
            None => {
                tracing::error!("Handler not found for: {}", method_name);
                return;
            }
        };

        // Spawn handler task
        let payload = frame.payload_bytes();
        tokio::spawn(async move {
            if let Err(e) = handler.call(&payload, ctx.clone()).await {
                tracing::error!("Handler error for {}: {}", method_name, e);
                if !ctx.is_responded() {
                    let _ = ctx.error(&e.to_string()).await;
                }
            }
        });
    }
```

### Event emitting & shutdown

```rust
    /// Emit event to parent (fire-and-forget)
    pub async fn emit<T: Serialize>(&self, event: &str, data: &T) -> Result<(), ProcwireError> {
        let event_id = self.registry.get_event_id(event)
            .ok_or_else(|| ProcwireError::UnknownEvent(event.to_string()))?;

        let payload = MsgPackCodec::serialize(data)?;

        let header = FrameHeader {
            method_id: event_id,
            flags: flags::DIRECTION_TO_PARENT, // Event, no IS_RESPONSE
            request_id: 0, // Events have no request ID
            payload_length: payload.len() as u32,
        };

        let header_bytes = encode_header(&header);
        let mut writer = self.pipe_writer.lock().await;
        writer.write_all(&header_bytes).await?;
        writer.write_all(&payload).await?;
        writer.flush().await?;

        Ok(())
    }

    /// Wait for shutdown (pipe close or parent kill)
    pub async fn wait_for_shutdown(self) -> Result<(), ProcwireError> {
        let _ = self.shutdown_rx.await;
        Ok(())
    }
}
```

## Testy

- [ ] Client starts successfully (mock parent)
- [ ] $init is sent to stdout
- [ ] Frame dispatch calls correct handler
- [ ] ABORT signal cancels handler
- [ ] Event emit sends correct frame
- [ ] Graceful shutdown on pipe close

## Definition of Done

- [ ] `cargo test client` — PASS
- [ ] Klient startuje, wysyła $init, akceptuje połączenie
- [ ] Frame dispatch działa
- [ ] Event emit działa
- [ ] Graceful shutdown na zamknięcie pipe
- [ ] Doc comments

## Kontekst

- Client runtime jest sercem całego crate
- Musi obsługiwać wiele concurrent requestów (każdy w osobnym task)
- Backpressure musi być respektowane
- ABORT pozwala parentowi anulować in-progress requesty
