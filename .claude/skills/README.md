# Claude Skills — procwire-client

Project-level [Claude Code skills](https://docs.claude.com/en/docs/claude-code/skills)
that capture the API surface, wire-protocol details, and implementation quirks
of this crate. Claude loads a skill on demand when its `description` matches the
task, so deep reference material lives here instead of bloating `CLAUDE.md`
(which is always in context).

These complement — they don't replace — `CLAUDE.md`, `docs/01-PROTOCOL-SPEC.md`,
and the `///` docs in `src/`.

## Skills

| Skill                      | Use it when…                                                                 |
|----------------------------|------------------------------------------------------------------------------|
| `procwire-wire-protocol`   | encoding/decoding frames, editing `src/protocol/*` or `src/writer.rs`, debugging framing / byte order / flags / stream termination, ensuring byte-for-byte parity with the Node parent |
| `procwire-msgpack-interop` | choosing payload types, debugging "Node can't decode this", the `to_vec_named` rule, type mapping, ext types, raw/zero-copy codec |
| `procwire-client-api`      | adding/modifying a handler or event, wiring a worker's `main()`, sharing state into handlers, cancellable work, builder config |
| `procwire-control-plane`   | editing `src/control/*` or `src/transport/*`, the `$init`/heartbeat/`$shutdown` lifecycle, stdout-discipline, pipe paths |
| `procwire-auth`            | the optional data-plane AUTH handshake, `PROCWIRE_TOKEN`, constant-time token check, `run_auth_handshake` |
| `procwire-testing`         | before committing, adding tests, finishing a task (CI gate + `AGENT_MEMORY.md`/`docs/TASKS` housekeeping) |

## Conventions

- One directory per skill, each containing a `SKILL.md` with YAML frontmatter
  (`name`, `description`) + a markdown body.
- `name` matches the directory name (lowercase, hyphens).
- `description` is written so Claude can decide *when* to invoke the skill —
  it names the relevant files and trigger conditions.
- Keep skills accurate to the code. When the implementation changes, update the
  matching skill (and `AGENT_MEMORY.md`).
