# Procwire Protocol Specification v2.0

> Dokument referencyjny dla implementacji klienta Rust.
> Opisuje pełny protokół komunikacyjny między Parent (Node.js) a Client (Rust worker).

## 1. Przegląd Architektury

Procwire używa **dual-channel architecture** — dwa niezależne kanały komunikacyjne:

```
┌─────────────────────────────────────────────────────────────┐
│                      PARENT PROCESS (Node.js)               │
│                       @procwire/core (ModuleManager)        │
├─────────────────────────────────────────────────────────────┤
│                                                             │
│  Control Plane (stdio)          Data Plane (named pipe)     │
│  └── JSON-RPC 2.0              └── BINARNY PROTOKÓŁ        │
│      • $init (child → parent)      • 11-byte header        │
│      • $heartbeat (planned)        • Codec payload          │
│      • $shutdown (planned)         • Zero JSON              │
│                                                             │
└──────────────────────┬─────────────────────┬────────────────┘
                       │ stdio               │ Unix Socket /
                       │ (stdin/stdout)      │ Named Pipe
                       ▼                     ▼
┌─────────────────────────────────────────────────────────────┐
│                      CHILD PROCESS (Rust worker)            │
│                       procwire-client-rs                    │
├─────────────────────────────────────────────────────────────┤
│                                                             │
│  Control Plane                  Data Plane                  │
│  └── JSON-RPC handler           └── Binary frame handler    │
│      (stdout writer)                (pipe listener/reader)  │
│                                                             │
└─────────────────────────────────────────────────────────────┘
```

### Dlaczego dwa kanały?

| Aspekt          | Control Plane (stdio)    | Data Plane (pipe)       |
|-----------------|--------------------------|-------------------------|
| Transport       | stdin/stdout             | Unix Socket / Named Pipe|
| Protokół        | JSON-RPC 2.0             | Binary (11-byte header) |
| Użycie          | Handshake, lifecycle     | Dane użytkownika        |
| Częstość        | Rzadko (raz/sek max)     | Tysiące/sek             |
| Rozmiar         | < 1 KB                   | MB — GB                 |
| Wydajność       | Nieistotna               | KRYTYCZNA               |

**ZASADA KARDYNALNA:** Dane użytkownika NIGDY nie przechodzą przez stdio/JSON-RPC.
Kanał kontrolny NIGDY nie jest blokowany przez duże transfery danych.

---

## 2. Control Plane — JSON-RPC 2.0 przez stdio

### 2.1. Transport

- **Child → Parent:** `stdout` (każda wiadomość to jedna linia JSON zakończona `\n`)
- **Parent → Child:** `stdin` (jedna linia JSON per wiadomość, zakończona `\n`)
- **stderr:** Przekierowane na `inherit` (logi child trafiają bezpośrednio do terminala parenta)

### 2.2. Handshake: Wiadomość `$init`

To JEDYNA wiadomość kontrolna implementowana w v2.0. Child wysyła ją natychmiast po
uruchomieniu pipe listenera, aby poinformować parenta o gotowości.

**Kierunek:** Child → Parent (przez stdout)

```json
{
  "jsonrpc": "2.0",
  "method": "$init",
  "params": {
    "pipe": "/tmp/procwire-12345-a1b2c3d4.sock",
    "schema": {
      "methods": {
        "echo": { "id": 1, "response": "result" },
        "process": { "id": 2, "response": "stream" },
        "fire": { "id": 3, "response": "ack" }
      },
      "events": {
        "progress": { "id": 1 }
      }
    },
    "version": "2.0.0"
  }
}
```

#### Pola `$init`:

| Pole               | Typ      | Opis                                                       |
|--------------------|----------|-------------------------------------------------------------|
| `params.pipe`      | string   | Ścieżka do Unix Socket / Named Pipe (child nasłuchuje)     |
| `params.schema`    | object   | Schemat metod i eventów obsługiwanych przez child           |
| `params.version`   | string   | Wersja protokołu (`"2.0.0"`)                               |
| `schema.methods`   | object   | Mapa `nazwa → { id: number, response: ResponseType }`      |
| `schema.events`    | object   | Mapa `nazwa → { id: number }`                              |

#### ResponseType:

| Wartość    | Opis                                                        |
|------------|-------------------------------------------------------------|
| `"result"` | Jedna pełna odpowiedź                                      |
| `"stream"` | Seria chunków zakończona STREAM_END                         |
| `"ack"`    | Potwierdzenie odbioru (może zawierać małe dane)             |
| `"none"`   | Fire-and-forget (w praktyce child i tak wysyła ack)         |

#### Ścieżka pipe — generowanie:

```
Unix/macOS: /tmp/procwire-{PID}-{RANDOM_8_CHARS}.sock
Windows:    \\.\pipe\procwire-{PID}-{RANDOM_8_CHARS}
```

### 2.3. Wiadomość `$error` (opcjonalna)

Child może wysłać `$error` jeśli nie jest w stanie się zainicjalizować:

```json
{
  "jsonrpc": "2.0",
  "method": "$error",
  "params": {
    "message": "Failed to initialize: missing config"
  }
}
```

### 2.4. Przyszłe wiadomości kontrolne (jeszcze nie zaimplementowane w v2.0)

Zaplanowane, ale NIE WYMAGANE dla MVP klienta Rust:

- `$heartbeat` — ping/pong dla health check
- `$shutdown` — graceful shutdown request

---

## 3. Data Plane — Binary Protocol przez Named Pipe

### 3.1. Transport

- Child uruchamia **pipe listener** (Unix Domain Socket na Linux/macOS, Named Pipe na Windows)
- Parent **łączy się** do tego pipe po otrzymaniu `$init`
- Komunikacja jest **dwukierunkowa** (full-duplex) na jednym połączeniu socket

### 3.2. Wire Format — Frame

Każdy frame (wiadomość) składa się z nagłówka o stałej wielkości i payloadu o zmiennej długości:

```
┌──────────┬───────┬──────────┬──────────┬──────────────────────┐
│ Method ID│ Flags │ Req ID   │ Length   │ Payload              │
│ 2 bytes  │ 1 byte│ 4 bytes  │ 4 bytes  │ N bytes              │
│ uint16 BE│       │ uint32 BE│ uint32 BE│ (codec output)       │
└──────────┴───────┴──────────┴──────────┴──────────────────────┘

HEADER: 11 bytes (stałe, Big Endian)
PAYLOAD: N bytes (zmienna długość, format zależy od codec)
```

#### Offsety nagłówka:

| Offset | Rozmiar | Typ       | Pole          | Opis                           |
|--------|---------|-----------|---------------|--------------------------------|
| 0      | 2       | uint16 BE | Method ID     | ID metody/eventu (z schematu)  |
| 2      | 1       | uint8     | Flags         | Flagi (patrz sekcja 3.3)      |
| 3      | 4       | uint32 BE | Request ID    | ID korelacyjne req/resp        |
| 7      | 4       | uint32 BE | Payload Length| Długość payloadu w bajtach     |

**Stałe:**

| Stała                   | Wartość           | Opis                              |
|-------------------------|-------------------|-----------------------------------|
| `HEADER_SIZE`           | 11                | Stały rozmiar nagłówka            |
| `DEFAULT_MAX_PAYLOAD`   | 1,073,741,824     | 1 GB (konfigurowalny)             |
| `ABSOLUTE_MAX_PAYLOAD`  | 2,147,483,647     | ~2 GB (limit Node.js Buffer)      |
| `ABORT_METHOD_ID`       | 0xFFFF (65535)    | Zarezerwowany dla sygnału abort   |
| `HEADER_POOL_SIZE`      | 16                | Pula pre-alokowanych nagłówków    |

### 3.3. Flags Byte

```
Bit 0: DIRECTION_TO_PARENT  (0 = to child, 1 = to parent)
Bit 1: IS_RESPONSE          (0 = request/event, 1 = response)
Bit 2: IS_ERROR             (0 = ok, 1 = error)
Bit 3: IS_STREAM            (0 = single, 1 = stream chunk)
Bit 4: STREAM_END           (0 = more coming, 1 = final chunk)
Bit 5: IS_ACK               (0 = full response, 1 = ack only)
Bit 6-7: reserved (MUSZĄ być 0)
```

**Wartości flag jako bitmaski:**

| Flaga              | Wartość binarna | Hex  | Dec |
|--------------------|-----------------|------|-----|
| DIRECTION_TO_PARENT| 0b00000001      | 0x01 | 1   |
| IS_RESPONSE        | 0b00000010      | 0x02 | 2   |
| IS_ERROR           | 0b00000100      | 0x04 | 4   |
| IS_STREAM          | 0b00001000      | 0x08 | 8   |
| STREAM_END         | 0b00010000      | 0x10 | 16  |
| IS_ACK             | 0b00100000      | 0x20 | 32  |

### 3.4. Method ID i Request ID

**Method ID (2 bytes, uint16 BE):**
- Przydzielane dynamicznie podczas handshake — child definiuje mapowanie `nazwa → id` w schemacie
- ID = 0 jest **zarezerwowane** (nie używać)
- ID = 0xFFFF (65535) jest zarezerwowane dla **ABORT**
- Metody i eventy mają **oddzielne przestrzenie ID** (echo method id=1 i progress event id=1 nie kolidują, bo context rozróżnia: request z flags=0x00 vs event z flags=0x01 i requestId=0)

**Request ID (4 bytes, uint32 BE):**
- Unikalne w ramach sesji, generowane przez nadawcę
- Parent generuje ID dla requestów (inkrementalne od 1)
- Request ID = 0 oznacza **fire-and-forget / event** (bez korelacji)
- Ten sam request ID jest użyty w response/stream chunks do korelacji

### 3.5. Payload

- Format payloadu zależy od **codec** skonfigurowanego dla danej metody
- Domyślny codec: **MessagePack (MsgPack)**
- Dostępne codecs: MsgPack, Arrow (IPC Stream), Raw (pass-through)
- Payload o długości 0 jest dozwolony (np. STREAM_END, ACK bez danych)

---

## 4. Scenariusze Komunikacji (Transakcje)

### 4.1. Request → Response (typ `result`)

Najprostszy scenariusz: parent wysyła request, child odpowiada jedną wiadomością.

```
Parent                                  Child (Rust)
  │                                        │
  │─── [method=1, flags=0x00, req=42] ───►│  Request
  │    [payload: MsgPack encoded data]     │
  │                                        │  (child przetwarza...)
  │◄── [method=1, flags=0x03, req=42] ────│  Response
  │    [payload: MsgPack encoded result]   │  (DIRECTION_TO_PARENT | IS_RESPONSE)
  │                                        │
```

**Flags request (parent → child):** `0x00` (wszystkie bity = 0)
**Flags response (child → parent):** `0x03` = DIRECTION_TO_PARENT (0x01) | IS_RESPONSE (0x02)

### 4.2. Request → ACK (typ `ack`)

Child potwierdza odbiór requestu. Może zawierać małe dane (np. `{ received: true }`).

```
Parent                                  Child (Rust)
  │                                        │
  │─── [method=2, flags=0x00, req=43] ───►│  Request
  │    [payload: data]                     │
  │                                        │
  │◄── [method=2, flags=0x23, req=43] ────│  ACK
  │    [payload: optional ack data]        │  (TO_PARENT | IS_RESPONSE | IS_ACK)
  │                                        │
```

**Flags ACK:** `0x23` = DIRECTION_TO_PARENT (0x01) | IS_RESPONSE (0x02) | IS_ACK (0x20)

### 4.3. Request → Stream (typ `stream`)

Parent wysyła request, child odpowiada serią chunków zakończoną znacznikiem końca.

```
Parent                                  Child (Rust)
  │                                        │
  │─── [method=3, flags=0x00, req=44] ───►│  Request
  │    [payload: input data]               │
  │                                        │
  │◄── [method=3, flags=0x0B, req=44] ────│  Stream Chunk 1
  │    [payload: chunk 1 data]             │  (TO_PARENT | IS_RESPONSE | IS_STREAM)
  │                                        │
  │◄── [method=3, flags=0x0B, req=44] ────│  Stream Chunk 2
  │    [payload: chunk 2 data]             │
  │                                        │
  │◄── [method=3, flags=0x1B, req=44] ────│  Stream End (final)
  │    [payload: empty (0 bytes)]          │  (+ STREAM_END)
  │                                        │
```

**Flags stream chunk:** `0x0B` = DIRECTION_TO_PARENT (0x01) | IS_RESPONSE (0x02) | IS_STREAM (0x08)
**Flags stream end:** `0x1B` = powyższe + STREAM_END (0x10)

**WAŻNE:** Frame STREAM_END ma **pusty payload** (payloadLength = 0). Nie serializuj żadnych danych — to czysty marker końca streamu.

### 4.4. Error Response

Child może odpowiedzieć błędem na dowolny request.

```
Parent                                  Child (Rust)
  │                                        │
  │─── [method=1, flags=0x00, req=45] ───►│  Request
  │                                        │
  │◄── [method=1, flags=0x07, req=45] ────│  Error
  │    [payload: "Error message" encoded]  │  (TO_PARENT | IS_RESPONSE | IS_ERROR)
  │                                        │
```

**Flags error:** `0x07` = DIRECTION_TO_PARENT (0x01) | IS_RESPONSE (0x02) | IS_ERROR (0x04)
**Payload error:** Zakodowany string z komunikatem błędu (przez codec, domyślnie MsgPack).

### 4.5. Event (child → parent)

Child emituje event do parenta — fire-and-forget, bez request ID.

```
Parent                                  Child (Rust)
  │                                        │
  │◄── [method=1, flags=0x01, req=0] ─────│  Event
  │    [payload: event data]               │  (DIRECTION_TO_PARENT, requestId=0)
  │                                        │
```

**Flags event:** `0x01` = DIRECTION_TO_PARENT
**Request ID:** **0** (eventy nie mają korelacji request/response)
**Method ID:** ID eventu z schematu (przestrzeń ID eventów)

### 4.6. Abort (parent → child)

Parent może anulować in-progress request.

```
Parent                                  Child (Rust)
  │                                        │
  │─── [method=0xFFFF, flags=0x00, req=44]│  Abort
  │    [payload: empty (0 bytes)]     ───►│  (ABORT_METHOD_ID, target requestId)
  │                                        │
  │    (child przerywa operację 44)        │
  │                                        │
```

**Method ID:** `0xFFFF` (zarezerwowany ABORT_METHOD_ID)
**Request ID:** ID requestu do anulowania
**Payload:** Pusty (0 bytes)

Child po otrzymaniu abort powinien:
1. Oznaczyć request jako aborted
2. Wywołać zarejestrowane callbacki abort
3. NIE wysyłać już response dla tego requestu (chyba że handler zdążył odpowiedzieć)

---

## 5. Pełny Lifecycle Sesji

```
┌─────────┐                              ┌──────────┐
│ Parent   │                              │ Child    │
│ (Node)   │                              │ (Rust)   │
└────┬─────┘                              └────┬─────┘
     │                                         │
     │  1. Parent spawnuje child process       │
     │  (stdio: pipe, stderr: inherit)         │
     │─────────────────────────────────────────►│
     │                                         │
     │         2. Child uruchamia się:         │
     │         - Rejestruje handlery metod     │
     │         - Rejestruje eventy             │
     │         - Przydziela method/event IDs   │
     │         - Tworzy pipe listener          │
     │         (Unix Socket / Named Pipe)      │
     │                                         │
     │◄────── 3. Child wysyła $init (stdout) ──│
     │         { pipe: path, schema: {...} }   │
     │                                         │
     │  4. Parent waliduje schemat             │
     │     (sprawdza czy child ma wymagane     │
     │      metody i eventy)                   │
     │                                         │
     │  5. Parent łączy się do pipe (connect)  │
     │─────────────────────────────────────────►│
     │                                         │
     │  6. Sesja READY — binary communication  │
     │◄════════════════════════════════════════►│
     │     Request/Response/Stream/Events      │
     │     (Binary Protocol na pipe)           │
     │                                         │
     │  7. Shutdown (parent kills process)     │
     │─────X                                   │
     │                                         │
```

### Kluczowe punkty lifecycle:

1. **Child tworzy pipe listener PRZED wysłaniem $init** — parent musi mieć dokąd się podłączyć
2. **$init idzie przez stdout** (control plane) — to jednorazowa wiadomość
3. **Parent waliduje schemat** — sprawdza czy child zarejestrował oczekiwane metody
4. **Parent łączy się do pipe** — i od tego momentu cała komunikacja danych jest binarna
5. **Shutdown** — parent zabija proces; w przyszłości będzie graceful shutdown przez `$shutdown`

---

## 6. Schema Validation

Parent definiuje oczekiwane metody i eventy w swojej konfiguracji.
Child definiuje swoje metody i eventy w `$init`.

**Parent sprawdza:**
- Czy KAŻDA metoda zdefiniowana w parent istnieje w schemacie child
- Czy KAŻDY event zdefiniowany w parent istnieje w schemacie child
- Jeśli brakuje metody/eventu → spawn FAILUJE z błędem

**Method ID mapping:**
- IDs są przydzielane przez child (kolejno od 1)
- Parent buduje lookup map `name → id` na podstawie schematu z `$init`
- Podczas wysyłania, parent tłumaczy `"echo"` → `1` (z lookup map)
- Podczas odbierania, parent tłumaczy `1` → `"echo"` (z reverse lookup map)

---

## 7. Backpressure

### Producent szybszy niż konsument

Jeśli child (Rust) wysyła dane szybciej niż parent jest w stanie je przetworzyć,
system operacyjny buforuje dane w pipe. Gdy bufor OS się zapełni:

- **Unix:** `write()` zwraca `EAGAIN` (non-blocking) lub blokuje (blocking)
- **Windows Named Pipe:** Similar behavior

### Implementacja w kliencie Rust

Child MUSI obsługiwać backpressure:
1. Sprawdzaj wartość zwracaną przez `write()`
2. Jeśli bufor jest pełny, poczekaj na drain event (writable readiness)
3. Nigdy nie buforuj nieograniczonej ilości danych w pamięci

### Implementacja w parent (Node.js)

Parent używa `DrainWaiter` — singleton pattern dla oczekiwania na drain:
- `socket.write()` zwraca `false` gdy bufor kernel jest pełny
- `DrainWaiter.waitForDrain()` czeka na event `drain`
- Wiele jednoczesnych requestów współdzieli jeden listener (bez MaxListenersExceededWarning)

---

## 8. Performance Considerations dla Rust Client

### 8.1. Zero-Copy

- **NIE kopiuj payloadu** przy parsowaniu nagłówka — użyj referencji/slice
- **Chunk accumulation:** Dane przychodzą z socketa w fragmentach (np. 64KB), nie łącz ich w pętli (`O(n²)`) — trzymaj `Vec<bytes>` i łącz lazy
- Prefer `bytes::Bytes` crate dla zero-copy slicing

### 8.2. Header Ring Buffer

Node.js implementacja używa ring buffer 16 pre-alokowanych buforów nagłówka.
Rust client POWINIEN zrobić to samo:
- Pre-alokuj tablicę 16 buforów po 11 bytes
- Round-robin index: `idx = (idx + 1) % 16`
- Eliminuje alokacje na hot path

### 8.3. Cork/Uncork (Nagle batching)

Node.js używa `socket.cork()` / `uncork()` aby zgrupować header+payload w jednym syscall.
Rust odpowiednik:
- Użyj `writev()` (scatter/gather I/O) do wysłania header+payload w jednym syscall
- Lub: `TCP_CORK` socket option na Linuxie
- Lub: `BufWriter` z explicit flush

### 8.4. Cel wydajnościowy

- **Throughput > 1 GB/s** dla dużych payloadów (10MB+)
- **Latency < 1ms** dla małych wiadomości (< 1KB)

---

## 9. Codec: MessagePack (domyślny)

### Format

- Standard MsgPack (https://msgpack.org)
- Extension types zdefiniowane w Node.js wersji:
  - Type 1: Buffer (raw bytes)
  - Type 2: Date (float64 timestamp)
- Rust: Użyj `rmp-serde` crate z konfigurancją `named` (struct fields as map keys)

### Zgodność

Klient Rust MUSI być w stanie deserializować dane wysłane przez Node.js i odwrotnie.
To oznacza:
- Obiekty JS → MsgPack Map → Rust struct (via serde)
- Rust struct → MsgPack Map → JS object
- Buffers: w Rust to `Vec<u8>` lub `Bytes`
- Null: w Rust to `Option<T>::None`

---

## 10. Wymagania platformowe

| Platforma | Pipe type                  | Path format                             |
|-----------|----------------------------|-----------------------------------------|
| Linux     | Unix Domain Socket         | `/tmp/procwire-{pid}-{rand}.sock`       |
| macOS     | Unix Domain Socket         | `/tmp/procwire-{pid}-{rand}.sock`       |
| Windows   | Named Pipe                 | `\\.\pipe\procwire-{pid}-{rand}`        |

Child MUSI:
1. Wykryć platformę
2. Wygenerować odpowiednią ścieżkę pipe
3. Nasłuchiwać na niej jako serwer
4. Zaakceptować jedno połączenie od parenta
5. Komunikować się na tym połączeniu do zamknięcia
