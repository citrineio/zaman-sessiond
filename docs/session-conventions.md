# Session conventions and handoff

This document records the working discipline for developing `zaman-sessiond`
and hands off context between sessions. Update it at the end of every session.

## Session ritual

Every session has exactly one feature goal and produces exactly one version.
No session bundles two features. No session ships half a feature to be finished
later under the same version number.

Order of operations:

1. **Recon.** Before writing code, confirm the current state of the tree:
   the files involved, their sizes, the current version, and whether any
   prerequisite from a prior session actually landed.
2. **Version bump.** Update every place the version appears (see checklist
   below). Do this first, so the change is visible in every subsequent diff.
3. **Test-first.** For each behavior change, write the failing test first,
   confirm it fails for the right reason, then implement.
4. **Build and test.** `cargo fmt`, `cargo build --locked`,
   `cargo test --locked`. Fix until green.
5. **Install and smoke test.** Build release binaries, back up installed
   binaries, stop services, install, start, verify version and status,
   run the live smoke test.
6. **Findings document.** Write `docs/session-<version>.md` while the
   session is fresh.
7. **Changelog.** Add an entry to `CHANGELOG.md`.
8. **Commit and push.** Only after install verification passes.

A session ends when the version is committed, pushed, and installed on the
Q6A. If install verification fails, the session is not over — fix in-session
or revert from backup and document why.

## Version bump checklist

Bumping the version touches three places. Miss one and `zamanctl version`
lies about what is running.

1. **`Cargo.toml`** — `version = "x.y.z"` at the top of the file.
2. **`src/contract.rs`** — `pub const VERSION: &str = "x.y.z";`. This is
   returned by the D-Bus `Version()` method and shown by `zamanctl version`.
3. **`Cargo.lock`** — updated automatically by `cargo build` after (1).

Verify after install:

```bash
zamanctl version   # must print the new number

