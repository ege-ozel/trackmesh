# Accepted MVP contract

This is the agreed product scope, including unfinished phases. Current deliverables and evidence are in [IMPLEMENTATION.md](IMPLEMENTATION.md); this page does not imply these features are all built.

Foundation phases 1–8 name the original implementation. Aşama 1 M0–M10 names the current direct-P2P plan, and #NN identifies review/UX tasks. The original infrastructure scope included relay code, now kept unbuilt in `future/`; the product uses direct connections only and defers relay to [post-1.0](plans/post-1.0.md#relay).

## Product and priorities

TrackMesh privately shares Ableton Live and FL Studio project folders between two or three trusted Windows collaborators. Priorities, in order: data integrity/security, maximum practical direct speed, reliability, simple workflow, development speed. Keep the MVP small; straightforward Rust code and explicit states are preferred.

```text
DAW → TrackMesh.vst3 → Windows Named Pipe → TrackMesh Agent / desktop
                                               ├ snapshot engine / SQLite / BLAKE3
                                               ├ identity / trusted peers / P2P transfer
                                               └ bounded metadata clients
                                                    ├ optional encrypted Supabase records
                                                    └ separate HTTPS metadata mailbox

Project files: collaborator PC ← direct P2P → collaborator PC
```

Desktop: Tauri 2, React, TypeScript, Rust. Core: Rust, Tokio, SQLite, BLAKE3, rust-libp2p. QUIC preferred; TCP + Noise/Yamux fallback. DCUtR, private rendezvous and circuit relay code exists from the foundation work; relay is outside the supported setup. Optional invite/endpoint/head service: per-user Supabase. Separate snapshot-announcement service: Rust, Axum, SQLite, HTTPS. VST3: smallest practical native C++ shim using the official Steinberg SDK; application logic stays in Rust.

## Identity and plugin

- Every enabled DAW project gets a permanent random UUID stored in VST3 state. Users manually link that UUID to a local folder once. No DAW `.als`/`.flp` path discovery hacks.
- Agent mappings include project UUID, folder, collaborators, latest local snapshot, and latest known remote snapshot. Generic identifiers such as `project_id`, `project_uuid`, `snapshot_id`, and `PeerId` retain their names.
- TrackMesh.vst3 stores UUID, connects to the local agent, displays status, exposes Send/Receive, and emits save/session signals. It does no scan, hash, SQLite, network, large disk I/O, metadata upload, or business logic.
- Windows Named Pipes carry small length-prefixed JSON. Events: `PLUGIN_CONNECTED`, `PROJECT_ID`, `SAVE_OBSERVED`, `SEND_SNAPSHOT`, `RECEIVE_SNAPSHOT`, `SESSION_ENDING`, `GET_STATUS`. The agent owns state.
- No expensive operation runs on the DAW audio thread. Plugin termination only queues `SESSION_ENDING` and returns promptly.

## Snapshots and files

- VST state serialization means save observed, not DAW file writing complete. Settle/debounce, scan, hash, then commit. No continuous filesystem watcher.
- Snapshots are immutable and contain snapshot/project/parent/author IDs, timestamp, manifest hash, relative files, sizes, BLAKE3 hashes/chunks, total size, changed bytes, and lineage.
- Start with internally configurable fixed 4 MiB chunks. No content-defined chunking.
- SQLite is authoritative local history. Snapshot creation is transactional; incomplete manifests never appear complete.
- Divergent children of the same parent are surfaced and block silent overwrite. No automatic conflict merging.
- A confirmed “Use collaborator's version” choice may replace a divergent working folder only after saving its current changes as a local snapshot; the prior files remain in the transfer backup, and this is never a silent overwrite.
- Receive only missing required chunks. Verify chunk BLAKE3 before durable completion; verify full files and fsync before atomic rename. Persist enough state to resume completed chunks and recover interrupted installation.
- Trusted peers have name, PeerId, application public key, and explicit trust. Reject unknown peers; discovery only locates already-authorized collaborators. Keep the existing `DIRECT` / `RELAYED` status values; `RELAYED` describes legacy capability outside the supported setup.

## Metadata mailbox and offline behavior

- The optional Supabase project holds small encrypted invite drops, endpoint records and head hints. A v2 invite link carries the service reference, publishable key and drop ID, with its decryption key in the fragment. The drop expires after 15 minutes and can be consumed once. Project files, samples and archives stay on collaborator PCs.
- Explicit approval queues a Supabase membership grant automatically. Only the device that created the project attempts ownership claim; imported projects carry a persisted marker. Creator-side Revoke queues membership revocation and retries when the service returns. Revocation does not rotate the project key, so a removed device keeps files and keys already received.
- Supabase head hints can show that an offline collaborator has a newer version. They do not establish current equality or make files downloadable from an offline PC. The self-hosted snapshot-announcement flow below is separate.
- Generate persistent Ed25519 device identity; protect private material with Windows credential protection/DPAPI. Authorized collaborators share a project encryption key.
- Encrypt snapshot metadata client-side with authenticated encryption, such as XChaCha20-Poly1305, and sign the envelope. Treat the mailbox as untrusted opaque storage.
- Records include opaque project/snapshot/author IDs, creation time, encrypted payload, and signature. Payload may describe parent, manifest hash, changed bytes, file count, and description. Target KB-level records; never send project files, samples, or archives to the mailbox.
- Commit locally → encrypt/sign → POST → server ACK → mark `PUBLISHED`. Failures stay `PENDING`, visible and retried while the agent is active.
- An offline author leaves an announcement in the mailbox while actual data stays on their PC. Show newer snapshot, author/time, and source offline. Files become available when an authorized source is online.
- Offline status is last-known history plus last verification. Unconditional `UP TO DATE` requires fresh verified equality with a contacted peer.

## UI and lifetime

- Minimal desktop project card: name, local and remote version status, collaborator availability, Share current version, Get collaborator's version, transfer state, and explicit divergence choices.
- Current Desktop setup is Link folder → Invite collaborator → open or Paste invite link → Accept → automatic signed device request with capability proof → explicit Approve. Approval shares the first snapshot when the inviter has files and no snapshot. The first version into an empty, available, never-received folder downloads automatically; every later receive needs Get collaborator's version and the DAW-closed confirmation. Divergence requires Use …'s version or Keep mine, with prior files backed up before replacement.
- Local collaborator rows, selected-peer connection status, pending enrollment and fresh equality remain distinct. Initial addresses come from the signed invitation; authenticated endpoint updates help existing collaborators reconnect. The first Invite or Accept asks for UPnP consent; manual port forwarding is under Settings → Advanced. Invitation files and CLI join/request-pairing remain Advanced paths; manual card exchange is legacy. Shared troubleshooting lives in [setup/README.md](setup/README.md). No public directory, browser deployment, offline pairing queue, permanent background service or automatic merge exists.
- VST view is smaller: project, local/remote snapshot status, peer availability, controls. The product name is TrackMesh; only the wordmark is drawn in capitals.
- Plugin launches TrackMesh Agent when needed. The agent remains alive for any VST session, pending scan/hash/commit, pending publication, or active transfer. When all work finishes, a grace period precedes clean exit.
- `SAVE_OBSERVED` followed by `SESSION_ENDING` must still finish local snapshot and metadata publication after the DAW closes. No permanent Windows background daemon initially.

## Phase order and exclusions

Foundation phases were built in this order: (1) workspace/storage/snapshots/identity; (2) trusted connectivity/resumable transfer; (3) encrypted mailbox + relay/rendezvous service code; (4) desktop; (5) VST3/IPC; (6) save-to-publication flow; (7) session-ending lifetime; (8) complete remote/offline/send/receive/divergence UX. The newer direct workflow follows [Aşama 1 M0–M10](plans/phase1-direct-p2p.md); separate-network acceptance remains unfinished.

Excluded: automated test/benchmark suites; cloud project-file storage or offline file delivery; continuous folder sync/watchers; public sharing, DHTs/swarms, IPFS, BitTorrent; mobile/macOS/Linux desktop; organizations/web dashboard; automatic conflict merging; content-defined chunking; automatic DAW path discovery.

The final workflow: A saves and publishes tiny encrypted metadata, then shuts down. B later sees a newer version and A offline. A returns online, B receives missing project data directly, hashes verify, and both peers acknowledge the same snapshot. Actual DAW save hooks and separate-network acceptance require the validation recorded in IMPLEMENTATION.md.
