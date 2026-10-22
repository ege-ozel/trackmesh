# TrackMesh agent entry point

## Read this first

- Canonical repository: `D:\CodeSpace\track-mesh`. Work in this repository; do not edit another checkout by assumption.
- Current product: Windows Rust CLI, Tauri 2/React desktop, C++ VST3/Named Pipe surface, and separate infrastructure services. Foundation phases 1–8 are implemented locally; Aşama 1 M0–M10 is the direct-P2P plan, and #NN identifies review/UX tasks. Foundation phase 5 has SDK editor-host validation; phase 6 has simulated save/loopback ACK validation; phase 8 has disposable offline-history/divergence UI and IPC validation. Supabase invitation/pairing/first-receive checks are recorded for one host plus Windows Sandbox. Actual DAW UUID persistence/audio/save hooks, separate-network transfer, self-hosted HTTPS deployment and newer Supabase changes remain unverified; consult docs/IMPLEMENTATION.md for exact evidence.
- Priorities: integrity/security → practical direct-transfer speed → reliability → simple workflow → development speed.
- Preserve architecture, protocol names, signed serialization, database values, and recovery ordering unless the task explicitly changes them.
- No automated tests, benchmarks, filesystem watchers, continuous sync, cloud project files, public sharing/discovery, automatic project conflict merging, or non-Windows desktop work in this MVP.

## Find the task owner

Use [docs/CODEMAP.md](docs/CODEMAP.md) to select files. Read the local `AGENTS.md` in the owning crate, then the relevant implementation. Avoid reading every crate or the entire documentation tree.

| Task | Start here |
| --- | --- |
| Two-PC setup, sender/peer instructions, trial package agent template | [docs/setup/README.md](docs/setup/README.md), then `docs/setup/host/README.md` or `docs/setup/peer/README.md`; package template: `docs/setup/peer/CLAUDE.md` |
| Receive/resume/installation/recovery | `crates/transfer/AGENTS.md` |
| SQLite/history/trust/publication state | `crates/storage/AGENTS.md` |
| Connectivity/discovery/transport | `crates/network/AGENTS.md` |
| Shared application workflow or CLI | `crates/core/AGENTS.md` |
| Desktop window, native documents, controls/status | `apps/desktop/AGENTS.md` |
| Native Windows VST3, UUID state, plugin IPC | `plugins/vst3/AGENTS.md`, `crates/core/AGENTS.md` |
| Supabase HTTPS client | `crates/supabase/AGENTS.md` |
| Post-1.0 mailbox/relay clients and services (not built) | `future/README.md` |
| Identity/signatures/encryption | `crates/crypto/src/lib.rs` and `shared/protocol/src/lib.rs` |
| Scan/hash/path safety/local chunks | `crates/snapshot/src/lib.rs` |
| Next MVP phase or scope question | [docs/IMPLEMENTATION.md](docs/IMPLEMENTATION.md), then the relevant section of [docs/MVP.md](docs/MVP.md) |

## Working rules

- Keep `lib.rs` files as API/ownership maps. Public APIs are re-exported at crate roots; private modules hold implementation.
- Put reusable project decisions in `core`; CLI, desktop, and future IPC are adapters. The VST3 bridge is a tiny control/status surface.
- One network actor owns the swarm. Blocking disk work stays outside it and outside any audio callback.
- Snapshot creation and transfer installation are different operations. Preserve their locks, transactional commits, and failure states.
- `TransferState` owns session labels; `ApplyPhase` owns journal phases. Do not duplicate either with string literals.
- Add dependencies or abstractions only when the assigned behavior requires them.
- Never print device secrets, decrypted project keys, or invitation payloads. The invitation is a private capability-bearing document.
- Do not change or delete user project folders, local databases, chunk stores, or recovery backups as cleanup.
- #45 authorizes guarded automatic retention of obsolete application chunks in the product, and explicit completed-backup deletion only through Settings → Storage, preserving each project's newest backup. It does not authorize Codex to clean production data while developing. When Codex is unsure which data would be deleted, stop and ask the user before deletion.
- Update the code map when moving modules. Update phase status only when the deliverable works; mark unverified integrations explicitly.
- Keep setup instructions under `docs/setup/host` and `docs/setup/peer`, with shared order/troubleshooting in `docs/setup/README.md`. Root `CLAUDE.md` routes repository development; `docs/setup/peer/CLAUDE.md` is the standalone peer-package template. Keep package instructions usable without the repository and private invitations outside agent context.
- Default Desktop setup uses Link folder → Invite collaborator → open or paste the one-read, 15-minute v2 link → automatic signed device request → explicit Approve → automatic first version into an empty, never-received folder → Share current version / Get collaborator's version. Optional per-user Supabase stores small encrypted invite drops, endpoint records and head hints. Approval automatically queues a membership grant; creator-side Revoke queues a membership revoke without rotating the project key. Native invitation files and CLI join/request-pairing are Advanced paths; keep manual card exchange labeled as legacy. Shared setup documents own discovery, availability limits, IP-change handling and bidirectional existing connections. Do not claim a public directory, browser deployment, offline pairing queue, permanent background service, automatic merge, relay fallback or hole punching is implemented; mailbox and relay client/server code is kept unbuilt in `future/` for post-1.0.
- Use a separate local `codex/phase-N-description` branch for each phase. After validation, commit and merge locally into `master` with `--no-ff`, then push `master` normally. Keep phase branches local; do not force push.

## Checks and handoff

Build/static-check commands and Windows runtime requirements are in [docs/DEVELOPMENT.md](docs/DEVELOPMENT.md). Do not add or run test/benchmark suites. Report which checks actually ran; historical manual checks are not evidence for a later change.

For a handoff, state the task, affected modules, invariants preserved, checks run, remaining work, and next concrete action. Keep `docs/IMPLEMENTATION.md` as the authoritative progress record; do not create competing status documents. It is a one-page status plus short new entries; the old log lives in `docs/history/` and is only searched (grep a task, milestone or date), never read whole.
