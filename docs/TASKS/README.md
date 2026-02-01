# Task Management

Ten folder zawiera taski projektu procwire-client-rs.

## Struktura

```
docs/tasks/
├── todo/           # Taski do zrobienia
├── done/           # Ukończone taski
└── README.md       # Ten plik
```

## Konwencje

### Format nazw plików

```
TASK-XX-krótki-opis.md
```

### Format pliku taska

```markdown
# TASK-XX: Tytuł

## Cel

Co chcemy osiągnąć.

## Zakres

- [ ] Subtask 1
- [ ] Subtask 2

## Implementacja

Szczegóły implementacji, kod, diagramy.

## Testy

- [ ] Test case 1
- [ ] Test case 2

## Definition of Done

- [ ] Kryterium 1
- [ ] Kryterium 2

## Kontekst

Dlaczego to robimy, powiązane pliki, decyzje.
```

## Workflow

1. **Nowy task** → utwórz w `todo/`
2. **W trakcie** → zaznacz checkboxy w zakresie
3. **Ukończony** → przenieś do `done/`
4. **Aktualizuj AGENT_MEMORY.md** → dodaj wpis o ukończeniu

## Lista tasków

### Phase 0: Bootstrap
- TASK-00: Inicjalizacja repozytorium
- TASK-00b: Weryfikacja CLAUDE.md

### Phase 1: Protocol Foundation
- TASK-01: Wire format (header encode/decode)
- TASK-02: Frame buffer (akumulacja z chunków)
- TASK-03: Frame struct + exports

### Phase 2: Codec Layer
- TASK-04: Codec trait + Raw codec
- TASK-05: MsgPack codec

### Phase 3: Transport Layer
- TASK-06: Pipe listener (Unix/Windows)
- TASK-07: Stdio I/O
- TASK-08: $init message builder

### Phase 4: Handler System
- TASK-09: Handler registry
- TASK-10: RequestContext

### Phase 5: Client Assembly
- TASK-11: Client builder
- TASK-12: Client runtime loop

### Phase 6: Testing & CI
- TASK-13: Unit test suite
- TASK-14: E2E tests z Node.js
- TASK-15: Examples
- TASK-16: CI/CD

## Dependency Graph

```
TASK-00 ──► TASK-00b
    │
    ▼
TASK-01 ──► TASK-02 ──► TASK-03
                            │
    ┌───────────────────────┘
    ▼
TASK-04 ──► TASK-05
    │
    ▼
TASK-06 ──► TASK-07 ──► TASK-08
    │                       │
    └───────────┬───────────┘
                ▼
          TASK-09 ──► TASK-10
                │         │
                └────┬────┘
                     ▼
               TASK-11 ──► TASK-12
                     │
              ┌──────┼──────┐
              ▼      ▼      ▼
         TASK-13  TASK-14  TASK-15
              │      │      │
              └──────┼──────┘
                     ▼
                  TASK-16
```
