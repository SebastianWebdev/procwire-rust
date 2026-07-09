# TASK-17: Kompatybilność błędów streamingu z procwire Phase-5 (PR #47)

## Cel

Dostosować klienta Rust do zaktualizowanych kontraktów client/parent z repo
`SebastianWebdev/procwire` (branch `claude/inspiring-goodall-gliGU`, PR #47),
wg `docs/rust-client-compatibility.md` — w szczególności §4.6 (obsługa błędów
przy streamingu).

## Zakres

- [x] Audyt checklisty §4 przewodnika względem stanu repo (v1.1.1 / Phase 4)
- [x] `flags::STREAM_ERROR_RESPONSE = 0x0F` w `wire_format.rs`
- [x] `RequestContext` zna `ResponseType` metody; `error()`/`error_with()`
      tagują ramki błędów metod `stream` flagą `IS_STREAM` (0x0F zamiast 0x07)
- [x] Ramka błędu terminalna: guard pojedynczej odpowiedzi terminalnej
      (`respond`/`ack`/`end`/`error` → `ProcwireError::ResponseAlreadySent`
      przy drugiej próbie; stan dzielony między klonami kontekstu)
- [x] Auto-wysyłka ramki błędu, gdy handler zwróci `Err` bez wysłanej
      odpowiedzi (w tym `HandlerNotFound` i błąd dekodowania payloadu) —
      parytet z Node `_handleFrame`/`_sendErrorResponse`
- [x] Testy: flagi 0x07/0x0F, payload MsgPack, terminalność, auto-error,
      brak dodatkowej ramki po już wysłanej odpowiedzi
- [x] Dokumentacja: `docs/01-PROTOCOL-SPEC.md` §4.4, README, skille
      `procwire-wire-protocol` i `procwire-client-api`

## Kontekst

Audyt wykazał, że pozycje REQUIRED §4.1 (heartbeat `$ping`/`$pong`),
§4.4 (limit `payloadLength` + walidacja D6) i §4.9 (AUTH `0xFFFE` +
`PROCWIRE_TOKEN`) oraz RECOMMENDED §4.2/§4.3/§4.5/§4.7 były już
zaimplementowane w v1.1.1 (Phase 4). Jedyną luką był §4.6:

1. `ctx.error()` wysyłał zawsze `0x07` — parent kierował błąd streamu do
   tabeli requestów, lookup chybiał, `for await` konsumenta wisiał na zawsze
   (streamy nie mają timeoutu).
2. Handler zwracający `Err` był tylko logowany — parent czekał 30 s (request)
   albo w nieskończoność (stream).
3. Brak guardu pozwalał wysłać `end()` po `error()` (kontrakt: stream kończy
   **albo** STREAM_END, **albo** ramka błędu — nigdy obie).

Wzorzec referencyjny: `packages/runtime-core/src/request-context.ts`
(`RequestContextImpl.error`, `_ensureNotResponded`) i
`packages/runtime-core/src/client-core.ts` (`_handleFrame`,
`_sendErrorResponse`).

## Definition of Done

- [x] `cargo fmt -- --check`
- [x] `cargo clippy --all-targets -- -D warnings`
- [x] `cargo test`
- [x] `cargo build --release`
- [x] AGENT_MEMORY.md zaktualizowane
