# Contributing to VirtualSSD

Thanks for taking the time to contribute. VirtualSSD is a real storage
project, and the people who make it better are the ones who run it against
their own drives, their own providers, and their own failure scenarios.

This document covers everything you need to get a change merged.

---

## Table of contents

- [Ways to contribute](#ways-to-contribute)
- [Before you start](#before-you-start)
- [Development setup](#development-setup)
- [The checks every change must pass](#the-checks-every-change-must-pass)
- [Coding conventions](#coding-conventions)
- [Commit messages](#commit-messages)
- [Pull requests](#pull-requests)
- [Adding a provider adapter](#adding-a-provider-adapter)
- [Reporting a bug](#reporting-a-bug)
- [Security issues](#security-issues)
- [Code of conduct](#code-of-conduct)

---

## Ways to contribute

You do not have to write Rust to help.

| Contribution | What it means | Good for |
|---|---|---|
| **Reproduce and report a bug** | Install the app, hit a problem, write down exactly what you did | This is the single most valuable contribution. We cannot test against real Google Drive and Telegram accounts. |
| **Answer a question in an issue** | Someone is stuck; you know the answer | Newcomers are especially good at this — you remember what was confusing |
| **Improve the documentation** | Fix a wrong statement, clarify a step, add a screenshot | The docs are the first thing a new user reads |
| **Add a provider adapter** | Implement S3, OneDrive, Backblaze, or a WebDAV backend | A well-scoped, self-contained piece of work |
| **Improve the UI** | Anything in `apps/desktop/src` | |
| **Work on the roadmap** | VSS snapshotting, auto-mirror, cross-process locking, BAT parsing | The big items are all broken down in `docs/performance-and-robustness.md` |
| **Write code** | Fix an issue, harden a path, add a test | |

If you are unsure where to start, open an issue describing what you want to
work on. That is not a wasted contribution — it is a request for direction.

## Before you start

- **Check the issue tracker.** Someone may already be working on it.
- **Open an issue first for anything substantial.** A design that is agreed
  before it is written costs far less than a pull request that has to be
  rewritten.
- **Read the docs.** [`docs/architecture.md`](docs/architecture.md) explains
  why the project uses the Windows Virtual Disk API instead of a kernel
  driver, and that decision constrains most of the design.

## Development setup

**Requirements**

- Windows 11 x64 or Windows 10 x64 (build 19044+)
- Rust 1.82 or newer — `rustup default stable`
- Node.js 20+ and `pnpm` — `npm install -g pnpm`
- The Microsoft Edge WebView2 Runtime (preinstalled on current Windows)

**Get the code**

```bash
git clone https://github.com/ahmad-abdullah-chaudary/virtualssd.git
cd virtualssd
pnpm install
```

**Run it**

```bash
cd apps/desktop
pnpm tauri dev
```

The app starts unelevated so you can add a provider and read the diagnostics
without administrator rights. Use **Restart as administrator** when you need
to mount a volume — attaching a VHDX is a privileged Windows operation.

**Run the tests**

```bash
cargo test --workspace          # every Rust suite
cd apps/desktop && npx tsc --noEmit
```

## The checks every change must pass

CI runs all of these, and a pull request is not mergeable until they pass.

| Check | Command | What it protects |
|---|---|---|
| Compile everything | `cargo check --workspace --all-targets` | No target — including tests and benches — is broken |
| Rust tests | `cargo test --workspace` | Behaviour is unchanged |
| Frontend types | `npx tsc --noEmit` in `apps/desktop` | No type errors reach the bundle |
| Frontend build | `pnpm build` in `apps/desktop` | The production bundle builds |
| Formatting | `cargo fmt --all -- --check` | Consistent style |
| Lints | `cargo clippy --workspace --all-targets -- -D warnings` | No new lint debt |

If you change the Rust code, please run `cargo fmt --all` before you push.

### Tests are not optional

A change to the storage path without a test is not finished. The engine
already has three layers you can extend:

- **Unit tests** live next to the code in `#[cfg(test)] mod tests`.
- **Integration tests** live in `crates/storage-core/tests/`. See
  `transfer_concurrency.rs` for how to exercise the real engine against an
  in-memory provider — you do not need a network or real credentials to
  test a transfer path.
- **Provider unit tests** live in `crates/provider-*/src/lib.rs` and cover
  the parsing and error-classification logic that does not need a network.

## Coding conventions

**Rust**

- `rustfmt` defaults, no exceptions.
- `clippy` must be clean. If a lint is wrong, allow it locally with a
  comment explaining why.
- Errors are values, not panics. A user-facing failure must carry a
  message that says what failed and what the user can do about it.
- **Never log a secret.** Tokens, refresh tokens, and recovery keys must
  never reach `tracing`. There are redaction helpers in `crates/security`
  and `crates/provider-*` — use them.

**TypeScript and React**

- `tsc --noEmit` must pass with no `any` in new code.
- Keep the light-only theme. The colour tokens are `#18191d` (dark
  surface), `#d4f938` (accent), and `#f2f4f7` (canvas).
- The four-square brand mark in `Sidebar.tsx` is the source of truth for the
  application icon. If you change it, regenerate the icon formats:
  ```bash
  cd apps/desktop && npx tauri icon app-icon.svg
  ```

**Honesty is a hard requirement**

This project has one rule that overrides style preferences:

> **The UI must reflect backend truth.** Never fabricate a value, never show
> a success that did not happen, never hide a failure behind a placeholder.

Concretely:

- A provider that does not expose a quota reports "Not reported by Google" or
  "Not exposed by Bot API" and stores `0`. It does not invent a number.
- A log line must come from a real backend event. The Diagnostics page
  renders the `tracing` stream; it never generates placeholder entries.
- An operation that partially succeeded must say which part failed.
- If a feature is not implemented, the documentation and the UI must say so.

A reviewer will reject a change that makes the product look more capable than
it is.

## Commit messages

Use [Conventional Commits](https://www.conventionalcommits.org/):

```
<type>(<scope>): <subject>

<body>

<footer>
```

**Types:** `feat`, `fix`, `perf`, `refactor`, `docs`, `test`, `build`, `ci`,
`chore`.

**Examples**

```
feat(provider-gdrive): resume uploads from the persisted session offset

fix(manifest): keep the last known-good generation when a write is corrupt

perf(storage-core): upload chunks with bounded concurrency

docs(troubleshooting): explain the Telegram quota message
```

Keep the subject under 72 characters and in the imperative mood.

## Pull requests

1. **Open an issue first** for anything beyond a small fix, and link it.
2. **Branch from `main`**: `git checkout -b feature/short-description`.
3. **Keep the change focused.** One logical change per pull request. A
   refactor and a feature in the same pull request are hard to review and
   hard to revert.
4. **Write the tests** in the same pull request as the change.
5. **Update the documentation** if you changed behaviour. `CHANGELOG.md`
   has an `[Unreleased]` section — add your entry there.
6. **Fill in the pull request template.** Explain *why*, not just *what*.
7. **CI must be green.** Do not merge around a failure.

**A reviewer will be looking for**

- Correctness under failure. What happens if the network drops halfway
  through a chunk? If the manifest write fails? If the process is killed?
- No regression in honesty. Does any message now claim more than the
  backend proved?
- Tests that would actually catch the bug being fixed.
- Boundaries respected. The provider crates must not depend on the desktop
  crate, and the desktop crate must not depend on a specific provider.

## Adding a provider adapter

A new backend is a self-contained crate. The contract is in
`crates/provider-contract/src/lib.rs`.

1. Create `crates/provider-<name>/` with a `Cargo.toml` and a `src/lib.rs`.
2. Implement `CloudStorageProvider`:
   - `verify_connection` — prove the credentials work.
   - `get_quota` — return the real numbers, or `0` and let the UI say the
     provider does not report them.
   - `upload_chunk` / `download_chunk` — verify the checksum on both paths.
     Never return data you have not verified.
   - `delete_chunk` — must be idempotent.
   - `publish_manifest` / `fetch_manifest` / `delete_manifest` — the manifest
     is the remote source of truth. A manifest that is not discoverable
     means the disk is invisible to a fresh installation, so
     `publish_manifest` must fail loudly if it cannot be recorded.
3. Map HTTP status codes to `ProviderError` precisely. A rate limit is not
   an authentication failure is not a quota error.
4. Redact secrets from every error string.
5. Add the crate to the workspace in the root `Cargo.toml`.
6. Add unit tests for the parsing and classification logic.
7. Document it in `docs/provider-compatibility.md`.

## Reporting a bug

A good report is worth more than a good fix, because it prevents the next
one.

**Include**

- VirtualSSD version (shown in the sidebar footer) and Windows build.
- Which provider, and whether the disk is encrypted.
- What you did, step by step.
- What you expected, and what happened instead.
- The **live engine log** from the Diagnostics page around the failure. It
  contains the real backend events and no secrets. Copy the relevant lines.
- If the disk is in a bad state, the contents of
  `%LOCALAPPDATA%\VirtualSSD\disks\<disk-id>\manifest.json`.

**Never** include a bot token, a refresh token, a client secret, or a
recovery key. If you paste one, rotate it immediately.

Use the [issue templates](https://github.com/ahmad-abdullah-chaudary/virtualssd/issues/new/choose).

## Security issues

**Do not open a public issue for a security vulnerability.**

Report it privately through
[GitHub's security advisory form](https://github.com/ahmad-abdullah-chaudary/virtualssd/security/advisories/new),
or by contacting the developer directly. Include the reproduction steps and
the impact.

You will get an acknowledgement, and a fix or a mitigation plan, before any
public disclosure. See [SECURITY.md](SECURITY.md) for the supported versions
and the full policy.

## Code of conduct

Be decent to each other. Assume good faith, critique the code and not the
person, and help newcomers. Harassment of any kind is not tolerated.

Report a problem to the maintainers if it happens.

---

Thank you for contributing. Every issue reproduced, every doc fixed, and
every provider added makes this project measurably more trustworthy for
someone whose data depends on it.
