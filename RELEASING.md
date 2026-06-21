# Releasing `procwire-client`

Publishing is automated with [**release-plz**](https://release-plz.dev) — the
Rust-ecosystem equivalent of the [changesets](https://github.com/changesets/changesets)
flow used by the Node/Bun [`procwire`](https://github.com/SebastianWebdev/procwire)
repo. You never run `cargo publish` by hand.

## The flow

```
feature PR (conventional commits)  ──merge──▶  main
                                                 │
                         release-plz opens/updates a "release" PR
                         (bumps Cargo.toml version + rewrites CHANGELOG)
                                                 │
                                   you merge the release PR
                                                 │
                         release-plz publishes to crates.io
                         + pushes the git tag + creates a GitHub Release
```

This mirrors `procwire`'s `changesets/action`: the same automation both **opens
the version PR** and **publishes**, depending on whether there is an
unreleased version bump waiting on `main`.

Concretely, on every push to `main` the [`release-plz.yml`](.github/workflows/release-plz.yml)
workflow runs two jobs:

- **`release-plz-pr`** — looks at the conventional commits since the last
  release and opens (or updates) a single **release PR** titled like
  `chore: release v1.1.0`, containing the version bump and the generated
  changelog. This is the "summary PR".
- **`release-plz-release`** — checks whether the version in `Cargo.toml` is
  newer than the one on crates.io. It is a **no-op** until the release PR is
  merged; once merged, it runs `cargo publish`, pushes the `vX.Y.Z` tag, and
  creates the GitHub Release.

## Day-to-day

1. Open a PR. Use **[Conventional Commits](https://www.conventionalcommits.org/)**
   for the commit/PR title — this is what decides the next version:
   - `fix: …` → patch (`1.0.0` → `1.0.1`)
   - `feat: …` → minor (`1.0.0` → `1.1.0`)
   - `feat!: …` / a `BREAKING CHANGE:` footer → major (`1.0.0` → `2.0.0`)
   - `docs:`/`chore:`/`test:`/`ci:` … → no release on their own
2. Merge the PR into `main`.
3. release-plz opens/updates the **release PR**. Review it (version + changelog).
4. Merge the release PR → the crate is published to crates.io and a GitHub
   Release/tag is created automatically.

There are no changeset files to author (unlike the npm flow); the changelog is
generated from the commit messages, so write them well.

## One-time setup (required)

Until these are done, the workflow will run but cannot publish or open PRs.

1. **Trusted Publishing on crates.io (OIDC — no token secret).** Publishing uses
   [crates.io Trusted Publishing](https://crates.io/docs/trusted-publishing), the
   same idea as npm's OIDC trusted publishing that the Node `procwire` repo uses:
   instead of storing a long-lived API token, release-plz mints a short-lived
   (~30 min) token via GitHub's OIDC at publish time. As an **owner** of the
   `procwire-client` crate, open it on crates.io →
   **Settings → Trusted Publishing → Add a new publisher (GitHub)** and enter:
   - **Repository owner:** `SebastianWebdev`
   - **Repository name:** `procwire-rust`
   - **Workflow filename:** `release-plz.yml`
   - **Environment:** leave empty (or set one and add a matching `environment:`
     to the `release-plz-release` job for an extra approval gate).

   The `id-token: write` permission on the `release-plz-release` job (already set
   in [`release-plz.yml`](.github/workflows/release-plz.yml)) is what lets
   release-plz perform the OIDC exchange. **No `CARGO_REGISTRY_TOKEN` secret is
   required.**
2. **Let Actions open PRs.** Under
   **Settings → Actions → General → Workflow permissions**, enable
   **"Allow GitHub Actions to create and approve pull requests"**. Without this,
   `release-plz-pr` fails with *"GitHub Actions is not permitted to create or
   approve pull requests."*

## Notes & gotchas

- **First release will be `1.1.0`.** `1.0.0` is already on crates.io, and this
  branch adds the post-audit protocol work as a `feat`, so the first release PR
  release-plz opens will propose `1.1.0`. The `release` job stays a no-op until
  that PR is merged, so there is no risk of an accidental immediate publish.
- **CHANGELOG transition.** The repo's `CHANGELOG.md` was hand-maintained; from
  now on release-plz manages it. On the **first** release PR, fold the existing
  hand-written `[Unreleased]` notes into the generated version section (or just
  delete `[Unreleased]`, since its content becomes the new version) so the entry
  isn't duplicated.
- **CI on the release PR.** PRs opened with the default `GITHUB_TOKEN` do **not**
  trigger other workflows, so `ci.yml` will not auto-run on the release PR (the
  same caveat applies to `procwire`'s changesets PR). The release PR only bumps
  the version and changelog — no code change — so this is low-risk. If you want
  full CI on it, create a Personal Access Token (or GitHub App token) and use it
  as `GITHUB_TOKEN` in `release-plz.yml` instead of `secrets.GITHUB_TOKEN`.
- **`repository` URL.** `Cargo.toml` `repository`/`homepage` and the
  `CHANGELOG.md` link footer point at `…/procwire-rust` (the repo this code lives
  in), which is what release-plz uses for changelog/crates.io links. (Note:
  `CLAUDE.md` still refers to `procwire-client-rs` as the project's GitHub home —
  align that too if `procwire-rust` is the canonical public repo.)
- **Action version.** `release-plz/action@v0.5` tracks the latest `0.5.x`. Pin to
  an exact `@v0.5.NNN` if you prefer fully reproducible runs.
