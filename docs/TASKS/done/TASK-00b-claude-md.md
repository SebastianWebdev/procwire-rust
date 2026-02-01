# TASK-00b: CLAUDE.md — Instrukcje dla Agenta

## Cel

Weryfikacja i uzupełnienie pliku `CLAUDE.md` z instrukcjami dla agenta kodującego.

## Zakres

- [ ] Przegląd istniejącego `CLAUDE.md`
- [ ] Weryfikacja kompletności instrukcji
- [ ] Dodanie brakujących sekcji (jeśli potrzebne)

## Wymagana zawartość CLAUDE.md

### Sekcje obowiązkowe

1. **Project Overview** - opis projektu
2. **Architecture** - Dual-Channel Design
3. **Critical Rules** - zasady których NIGDY nie łamać:
   - Data Plane = BINARY ONLY
   - Control Plane = JSON only for $init
   - All numbers = Big Endian
   - Header = exactly 11 bytes
   - Method ID 0 = reserved
   - Method ID 0xFFFF = ABORT
   - Request ID 0 = event
4. **Wire Format** - diagram nagłówka
5. **Flags byte** - wszystkie bity
6. **Commands** - cargo check/test/clippy/fmt/bench
7. **Code Style** - thiserror, tracing, bytes, tokio
8. **Testing** - unit tests, integration, E2E

## Definition of Done

- [ ] CLAUDE.md istnieje w korzeniu repo
- [ ] Zawiera wszystkie krytyczne reguły protokołu
- [ ] Zawiera komendy budowania i testowania
- [ ] Zawiera instrukcje dotyczące code style

## Kontekst

- CLAUDE.md już istnieje (utworzony podczas inicjalizacji)
- Task polega na weryfikacji kompletności
- Jeśli wszystko jest OK - task można oznaczyć jako done
