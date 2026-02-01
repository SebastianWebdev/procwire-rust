# TASK-10: RequestContext

## Cel

Kontekst przekazywany do handlera — umożliwia wysyłanie odpowiedzi.

## Plik

`src/handler/context.rs`

## Zakres

- [ ] Implementacja `RequestContext` struct
- [ ] Metody respond/ack/chunk/end/error
- [ ] Obsługa backpressure (write_frame)
- [ ] Abort/cancellation support
- [ ] Testy jednostkowe

## Implementacja

### RequestContext

```rust
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::io::AsyncWriteExt;
use serde::Serialize;

use crate::protocol::{FrameHeader, encode_header, flags};
use crate::codec::MsgPackCodec;
use crate::error::ProcwireError;

pub struct RequestContext {
    request_id: u32,
    method_name: String,
    method_id: u16,
    writer: Arc<Mutex<Box<dyn AsyncWriteExt + Send + Unpin>>>,
    responded: AtomicBool,
    aborted: AtomicBool,
    // TODO: CancellationToken for cooperative cancellation
}

impl RequestContext {
    pub fn new(
        request_id: u32,
        method_name: String,
        method_id: u16,
        writer: Arc<Mutex<Box<dyn AsyncWriteExt + Send + Unpin>>>,
    ) -> Self { ... }

    /// Send full response (response type: "result")
    pub async fn respond<T: Serialize>(&self, data: &T) -> Result<(), ProcwireError> {
        self.ensure_not_responded()?;
        self.responded.store(true, Ordering::SeqCst);

        let payload = MsgPackCodec::serialize(data)?;
        let flags = flags::DIRECTION_TO_PARENT | flags::IS_RESPONSE;
        self.write_frame(flags, &payload).await
    }

    /// Send ACK with data (response type: "ack")
    pub async fn ack<T: Serialize>(&self, data: &T) -> Result<(), ProcwireError> {
        self.ensure_not_responded()?;
        self.responded.store(true, Ordering::SeqCst);

        let payload = MsgPackCodec::serialize(data)?;
        let flags = flags::DIRECTION_TO_PARENT | flags::IS_RESPONSE | flags::IS_ACK;
        self.write_frame(flags, &payload).await
    }

    /// Send ACK without data
    pub async fn ack_empty(&self) -> Result<(), ProcwireError> {
        self.ensure_not_responded()?;
        self.responded.store(true, Ordering::SeqCst);

        let flags = flags::DIRECTION_TO_PARENT | flags::IS_RESPONSE | flags::IS_ACK;
        self.write_frame(flags, &[]).await
    }

    /// Send stream chunk (response type: "stream")
    pub async fn chunk<T: Serialize>(&self, data: &T) -> Result<(), ProcwireError> {
        // chunk() NIE ustawia responded — można wysłać wiele chunków
        let payload = MsgPackCodec::serialize(data)?;
        let flags = flags::DIRECTION_TO_PARENT | flags::IS_RESPONSE | flags::IS_STREAM;
        self.write_frame(flags, &payload).await
    }

    /// End stream (sends STREAM_END frame with empty payload)
    pub async fn end(&self) -> Result<(), ProcwireError> {
        self.ensure_not_responded()?;
        self.responded.store(true, Ordering::SeqCst);

        // STREAM_END ma ZAWSZE pusty payload!
        let flags = flags::DIRECTION_TO_PARENT | flags::IS_RESPONSE
                  | flags::IS_STREAM | flags::STREAM_END;
        self.write_frame(flags, &[]).await
    }

    /// Send error response
    pub async fn error(&self, message: &str) -> Result<(), ProcwireError> {
        self.ensure_not_responded()?;
        self.responded.store(true, Ordering::SeqCst);

        let payload = MsgPackCodec::serialize(&message)?;
        let flags = flags::DIRECTION_TO_PARENT | flags::IS_RESPONSE | flags::IS_ERROR;
        self.write_frame(flags, &payload).await
    }

    /// Check if request was aborted
    pub fn is_aborted(&self) -> bool {
        self.aborted.load(Ordering::SeqCst)
    }

    /// Request ID
    pub fn request_id(&self) -> u32 { self.request_id }

    /// Method name
    pub fn method(&self) -> &str { &self.method_name }

    // Internal methods
    fn ensure_not_responded(&self) -> Result<(), ProcwireError> {
        if self.responded.load(Ordering::SeqCst) {
            return Err(ProcwireError::ResponseAlreadySent(self.request_id));
        }
        Ok(())
    }

    async fn write_frame(&self, flags: u8, payload: &[u8]) -> Result<(), ProcwireError> {
        let header = FrameHeader {
            method_id: self.method_id,
            flags,
            request_id: self.request_id,
            payload_length: payload.len() as u32,
        };

        let header_bytes = encode_header(&header);
        let mut writer = self.writer.lock().await;

        // Write header + payload
        // TODO: use writev for single syscall
        writer.write_all(&header_bytes).await?;
        writer.write_all(payload).await?;
        writer.flush().await?;

        Ok(())
    }
}

impl Clone for RequestContext {
    fn clone(&self) -> Self { ... }
}
```

## Flagi dla każdego typu odpowiedzi

| Metoda | Flags |
|--------|-------|
| `respond()` | `0x03` = DIRECTION_TO_PARENT \| IS_RESPONSE |
| `ack()` / `ack_empty()` | `0x23` = DIRECTION_TO_PARENT \| IS_RESPONSE \| IS_ACK |
| `chunk()` | `0x0B` = DIRECTION_TO_PARENT \| IS_RESPONSE \| IS_STREAM |
| `end()` | `0x1B` = DIRECTION_TO_PARENT \| IS_RESPONSE \| IS_STREAM \| STREAM_END |
| `error()` | `0x07` = DIRECTION_TO_PARENT \| IS_RESPONSE \| IS_ERROR |

## Testy

- [ ] respond() sets responded flag
- [ ] Double respond() returns error
- [ ] chunk() doesn't set responded
- [ ] end() sets responded
- [ ] error() sets responded
- [ ] Correct flags for each response type
- [ ] STREAM_END has empty payload

## Definition of Done

- [ ] `cargo test handler::context` — PASS
- [ ] Poprawne flagi dla każdego typu odpowiedzi
- [ ] Backpressure propagated (write_all handles partial writes)
- [ ] Doc comments

## Kontekst

- RequestContext jest przekazywany do każdego handlera
- Handler używa ctx do wysyłania odpowiedzi
- **KRYTYCZNE:** STREAM_END ma ZAWSZE pusty payload (payloadLength=0)
- Backpressure musi być obsłużone — nigdy nie ignoruj wyników write
