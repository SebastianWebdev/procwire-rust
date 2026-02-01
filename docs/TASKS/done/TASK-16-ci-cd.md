# TASK-16: CI/CD (GitHub Actions)

## Cel

Automatyczne testy i linting na CI.

## Plik

`.github/workflows/ci.yml`

## Zakres

- [ ] Workflow dla push/PR
- [ ] Matrix: Linux, macOS, Windows
- [ ] Matrix: Rust stable + 1.75 (MSRV)
- [ ] Format check (rustfmt)
- [ ] Lint (clippy)
- [ ] Unit tests
- [ ] Build examples
- [ ] E2E tests (z Node.js)

## Implementacja

### `.github/workflows/ci.yml`

```yaml
name: CI

on:
  push:
    branches: [main]
  pull_request:
    branches: [main]

env:
  CARGO_TERM_COLOR: always

jobs:
  # Format and lint
  check:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4

      - name: Install Rust
        uses: dtolnay/rust-toolchain@stable
        with:
          components: rustfmt, clippy

      - name: Check formatting
        run: cargo fmt -- --check

      - name: Clippy
        run: cargo clippy -- -D warnings

  # Tests on all platforms
  test:
    needs: check
    strategy:
      fail-fast: false
      matrix:
        os: [ubuntu-latest, macos-latest, windows-latest]
        rust: [stable, "1.75"]

    runs-on: ${{ matrix.os }}

    steps:
      - uses: actions/checkout@v4

      - name: Install Rust
        uses: dtolnay/rust-toolchain@master
        with:
          toolchain: ${{ matrix.rust }}

      - name: Cache cargo
        uses: Swatinem/rust-cache@v2

      - name: Build
        run: cargo build --all-features

      - name: Run tests
        run: cargo test --all-features

      - name: Build examples
        run: cargo build --examples

  # E2E tests with Node.js
  e2e:
    needs: test
    runs-on: ubuntu-latest

    steps:
      - uses: actions/checkout@v4

      - name: Install Rust
        uses: dtolnay/rust-toolchain@stable

      - name: Setup Node.js
        uses: actions/setup-node@v4
        with:
          node-version: "22"

      - name: Install procwire packages
        run: npm install -g @procwire/core @procwire/client @procwire/codecs @procwire/protocol

      - name: Build test worker
        run: cargo build --example test_worker

      - name: Run E2E tests
        run: cargo test --test e2e_test

  # Release build check
  release:
    needs: test
    runs-on: ubuntu-latest

    steps:
      - uses: actions/checkout@v4

      - name: Install Rust
        uses: dtolnay/rust-toolchain@stable

      - name: Release build
        run: cargo build --release

      - name: Check docs
        run: cargo doc --no-deps

  # Security audit
  security:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: rustsec/audit-check@v2
        with:
          token: ${{ secrets.GITHUB_TOKEN }}
```

### Opcjonalny workflow dla benchmarków

```yaml
# .github/workflows/bench.yml
name: Benchmarks

on:
  push:
    branches: [main]

jobs:
  bench:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4

      - name: Install Rust
        uses: dtolnay/rust-toolchain@stable

      - name: Run benchmarks
        run: cargo bench -- --noplot

      - name: Store results
        uses: actions/upload-artifact@v4
        with:
          name: benchmark-results
          path: target/criterion
```

## Wymagane secrets

- `GITHUB_TOKEN` - automatycznie dostępny

## Badge dla README

```markdown
[![CI](https://github.com/SebastianWebdev/procwire-client-rs/actions/workflows/ci.yml/badge.svg)](https://github.com/SebastianWebdev/procwire-client-rs/actions/workflows/ci.yml)
```

## Definition of Done

- [ ] CI zielony na wszystkich platformach
- [ ] Clippy zero warnings (`-D warnings`)
- [ ] Fmt check pass
- [ ] E2E tests pass (Linux)
- [ ] Badge w README

## Kontekst

- CI jest ostatnią linią obrony przed regresją
- Matrix testuje różne platformy i wersje Rust
- MSRV (Minimum Supported Rust Version) = 1.75
- E2E wymaga Node.js - uruchamiamy tylko na Linux
