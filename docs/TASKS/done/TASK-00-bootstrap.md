# TASK-00: Inicjalizacja Repozytorium i Środowiska

## Cel

Utworzenie kompletnego scaffoldu projektu Rust.

## Wymagania wstępne

- Rust toolchain (rustup): `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`
- Minimum Rust 1.75+ (edition 2021)
- Git zainstalowany

## Zakres

- [ ] Utworzenie projektu Cargo (`cargo init --lib`)
- [ ] Konfiguracja `Cargo.toml` z dependencies
- [ ] Utworzenie struktury katalogów
- [ ] Utworzenie placeholder plików dla wszystkich modułów
- [ ] Utworzenie `.gitignore`
- [ ] Weryfikacja (`cargo check`, `cargo test`)

## Kroki

### 1. Utwórz projekt Cargo

```bash
cargo init --lib
```

### 2. Skonfiguruj `Cargo.toml`

```toml
[package]
name = "procwire-client"
version = "0.1.0"
edition = "2021"
description = "Rust client SDK for Procwire IPC — high-performance binary protocol"
license = "MIT"
keywords = ["ipc", "binary-protocol", "process-communication"]
categories = ["network-programming"]
rust-version = "1.75"

[dependencies]
tokio = { version = "1", features = ["full"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
rmp-serde = "1"
bytes = "1"
tracing = "0.1"
thiserror = "2"

[dev-dependencies]
tokio-test = "0.4"
criterion = { version = "0.5", features = ["async_tokio"] }

[[bench]]
name = "throughput"
harness = false
```

### 3. Utwórz strukturę katalogów

```bash
mkdir -p src/protocol src/control src/transport src/codec src/handler
mkdir -p tests examples benches
```

### 4. Utwórz placeholder pliki

```
src/
├── lib.rs                # Public API exports
├── client.rs
├── error.rs
├── backpressure.rs
├── protocol/
│   ├── mod.rs
│   ├── wire_format.rs
│   ├── frame_buffer.rs
│   └── frame.rs
├── control/
│   ├── mod.rs
│   ├── init.rs
│   └── stdio.rs
├── transport/
│   ├── mod.rs
│   └── pipe.rs
├── codec/
│   ├── mod.rs
│   ├── msgpack.rs
│   └── raw.rs
└── handler/
    ├── mod.rs
    ├── context.rs
    └── registry.rs
```

### 5. Utwórz `.gitignore`

```
/target
Cargo.lock
*.swp
*.swo
.idea/
.vscode/
```

## Definition of Done

- [ ] `cargo check` przechodzi bez błędów
- [ ] `cargo test` przechodzi (puste testy)
- [ ] Struktura katalogów zgodna z planem
- [ ] Wszystkie placeholder pliki istnieją
- [ ] `.gitignore` istnieje

## Kontekst

- Ten task tworzy fundament dla wszystkich kolejnych tasków
- Struktura katalogów odpowiada architekturze opisanej w `docs/02-PACKAGE-PLAN.md`
- Dependencies są uzasadnione w dokumentacji (każdy crate ma swój powód)
