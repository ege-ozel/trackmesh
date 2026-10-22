# Code map

Paths are relative to repository root. A crate's `src/lib.rs` lists its public API and private modules. Start with the task owner and follow only the calls needed for the change.

## Task to file

| Task | Primary owner | Related boundary |
| --- | --- | --- |
| Two-PC setup, invitation-based discovery, panel/availability, IP changes and package agent instructions | `docs/setup/README.md`, `docs/setup/host/README.md`, `docs/setup/peer/README.md`, `docs/setup/peer/CLAUDE.md` | Root `AGENTS.md`/`CLAUDE.md` route repo work; `IMPLEMENTATION.md` owns validation history |
| Link folder, pair device, import/export invitation | `crates/core/src/lib.rs` | Snapshot, crypto, and storage APIs |
| Automatic pairing windows, capability challenge and explicit project approval | `crates/core/src/pairing.rs` | Native Desktop candidate controls and CLI `request-pairing`; no DB/schema change |
| Pairing-only inbound gate and bounded RPC frames | `crates/network/src/gate.rs`, `codec.rs`, `actor.rs`, `handle.rs` | Same swarm; transfer authorization and trusted discovery unchanged |
| Additive pairing messages and empty-message AEAD proof | `shared/protocol/src/pairing.rs`, `crates/crypto/src/pairing.rs` | Existing signed invite/card/transfer formats preserved |
| Public IPv4 endpoint validation/construction for adapters | `crates/core/src/connection.rs` | Native desktop invitation export; no swarm restart |
| Read-only Windows IPv4 interface, gateway, and route observation | `crates/core/src/network_observation.rs` | IP Helper calls run during Desktop startup in `spawn_blocking`; no router/firewall requests |
| Read-only Windows Firewall profile and candidate rule observation | `crates/core/src/firewall_observation.rs` | COM runs during long-lived Desktop startup in `spawn_blocking`; candidate rules do not prove effective permission |
| Versioned local opt-in and finite UPnP IGD mapping worker | `crates/core/src/settings.rs`, `crates/network/src/portmap.rs` | `PeerRuntime` owns worker shutdown; the network actor stays unblocked; no endpoint advertisement |
| Manual/settled snapshot creation, unchanged-content reuse and commit | `crates/core/src/snapshots.rs` | Project lock, snapshot scanner and transactional storage |
| Local status, compare with online peer, receive from peer, recover | `crates/core/src/workflows.rs` | `peer.rs`, transfer API |
| Bounded project/peer metadata cards, remote history and known lineage | `crates/core/src/status.rs` | Narrow bounded storage queries; existing CLI status unchanged |
| Manual snapshot commit | `crates/core/src/sending.rs` | Snapshot workflow |
| Windows plugin pipe, picker, binding, status, Share queue and Desktop handoff | `crates/core/src/ipc/mod.rs`, `ipc/security.rs`, `ipc/status.rs` | Existing plugin events with defaulted status fields; shared Engine/network; bounded Desktop request |
| Project save debounce, coalescing and accepted-work drain | `crates/core/src/ipc/jobs.rs` | Engine settled send; bounded per-project revisions |
| Default-off per-project DAW auto-share preference | `crates/core/src/auto_share.rs`, `settings.rs`, `ipc/mod.rs`, `ipc/jobs.rs` | Versioned atomic settings; Desktop/plugin share one preference; capability-gated local pipe setter |
| Advisory working presence, bounded folder metadata and quiet expiry | `crates/core/src/presence.rs`, `peer.rs`, `ipc/mod.rs` | One project-authorized additive peer request; memory-only state; Desktop card and VST3 warning/Share confirmation; no lineage decision |
| Work-based owner retention and 60-second idle grace | `crates/core/src/lifetime.rs` | Plugin/peer activity epochs; narrow authored-pending query |
| VST3 factory, audio pass-through, UUID state/signals | `plugins/vst3/src/plugin.cpp`, `plugin.h` | Official Steinberg SDK; Rust owns decisions |
| Native VST3 view, IPC worker and bundled-owner launch | `plugins/vst3/src/view.cpp`, `bridge.cpp` | No audio-thread IPC or file/network work |
| Windows desktop entry point, capability inventory, navigation | `apps/desktop/src-tauri/src/lib.rs`, `build.rs`, `capabilities/main.json` | Tauri adapter; local bundled React |
| Desktop jobs, status freshness, explicit session shutdown | `apps/desktop/src-tauri/src/runtime.rs` | Core workflows and one leased `PeerRuntime` |
| Native tray, reopen and confirmed Quit | `apps/desktop/src-tauri/src/tray.rs`, `runtime.rs`, `crates/core/src/lifetime.rs`, `settings.rs` | Cached Tauri tray feature; first-close notice; confirmed drain; same network owner |
| Native invitation/legacy card files and folder dialogs | `apps/desktop/src-tauri/src/documents.rs` | Automatic return uses pairing RPC; bounded private documents stay in Rust |
| Main-window command boundary and typed frontend | `apps/desktop/src-tauri/src/commands.rs`, `apps/desktop/src/api.ts` | Narrow invoke API |
| Desktop project controls and styling | `apps/desktop/src/App.tsx` (shell, rail, dialogs), `ProjectCard.tsx`, `AdvancedScreen.tsx`, `SettingsScreen.tsx`, `Jobs.tsx`, `format.ts`, `src/styles.css` | Core-derived state, no filesystem watcher |
| Automatic chunk retention and Storage backup cleanup | `crates/core/src/retention.rs`, `crates/storage/src/retention.rs`, `crates/snapshot/src/retention.rs`, `crates/transfer/src/backups.rs` | Shared Windows cache lease; immutable signed history; DONE-only deletion with newest backup protected; `apps/desktop/src/StorageSettings.tsx` |
| Get download consent | `crates/transfer/src/download.rs` (`DownloadGate`), `apps/desktop/src-tauri/src/receive.rs`, `crates/snapshot/src/disk.rs` | Verified distinct missing chunks; free-space refusal before download; Get/Use version always shows the change list, and that window is also the DAW-closed question; Use version first asks in the card, before it saves the folder as a version the collaborator can get (Recover keeps its own question); automatic first version asks above 1 GB or for program/script files; retry pins the same signed target |
| Start/stop peer runtime, answer disk requests, fetch signed ancestry | `crates/core/src/peer.rs` | Network actor and transfer serving |
| Signed endpoint record acceptance and direct-peer publication | `crates/core/src/endpoints.rs` | Shared direct/Supabase validation, M5b storage, network actor learned candidates |
| CLI arguments, JSON output, Ctrl+C | `crates/core/src/main.rs` | Core workflows; keep decisions out of this adapter |
| Receive decisions, local lock, failure state | `crates/transfer/src/receiver.rs` | Verification, download, apply |
| Chunk reuse, concurrency, retries, durable progress | `crates/transfer/src/download.rs` | Chunk store, network handle, storage transfers |
| Author/signature/project binding, preflight folder comparison | `crates/transfer/src/verification.rs` | Crypto and snapshot APIs |
| Serve head/manifest/chunk/verification requests | `crates/transfer/src/serving.rs` | Storage authorization and chunk index |
| Assemble/fsync files outside working folder | `crates/transfer/src/assemble.rs` | Chunk store |
| Durable phase orchestration | `crates/transfer/src/apply.rs` | Journal, installation, storage transactions |
| Backup originals, install files, verify final folder/commit head | `crates/transfer/src/installation.rs` | Snapshot path/file helpers |
| Journal format and atomic persistence | `crates/transfer/src/journal.rs` | Snapshot atomic replacement |
| Explicit interrupted-apply recovery | `crates/transfer/src/recovery.rs` | Apply and trusted target validation |
| SQLite open/schema migration/mutex | `crates/storage/src/lib.rs`, `src/schema.sql` | Domain storage modules; additive schema v3 endpoint migration is transactional |
| Persisted structs and row decoding | `crates/storage/src/models.rs` | `transfer_state.rs`, `endpoints.rs` |
| Durable local endpoint sequence and peer endpoint watermark/expiry | `crates/storage/src/endpoints.rs` | Schema v3 tables; no trust or network authorization |
| Six transfer labels and SQLite/JSON conversion | `crates/storage/src/transfer_state.rs` | Transfers and receiver/recovery |
| Project links and errors | `crates/storage/src/projects.rs` | Core |
| Device identity bindings and per-project authorization | `crates/storage/src/trust.rs` | Network/transfer requests |
| Immutable snapshot commit, indexed chunk authorization, lineage and bounded adapter history | `crates/storage/src/snapshots.rs` | Snapshot/apply |
| Session creation, progress, transactional apply markers | `crates/storage/src/transfers.rs` | Receiver/apply |
| Supabase endpoint mailbox and invite drop | `services/supabase/README.md`, `services/supabase/supabase/migrations/20261012000100_trackmesh_m5d.sql`, `20261012000200_m6_invite_drop.sql`, `supabase/functions/`, `crates/supabase/src/client.rs`, `crates/crypto/src/supabase.rs`, `crates/crypto/src/invite_drop.rs`, `crates/core/src/supabase.rs`, `crates/core/src/settings.rs` | Shared signed auth and invite types in `shared/protocol/src/`; Desktop native link/clipboard in `apps/desktop/src-tauri/src/{uri,clipboard}.rs`; project controls in `apps/desktop/src/ProjectCard.tsx`, `AdvancedScreen.tsx` |
| Supabase one-read invite drop and native invite link | `services/supabase/supabase/migrations/20261012000200_m6_invite_drop.sql`, `supabase/functions/invite-create/`, `invite-consume/`, `crates/crypto/src/invite_drop.rs`, `crates/supabase/src/client.rs`, `apps/desktop/src-tauri/src/clipboard.rs`, `uri.rs` | Signed ProjectInvite payload; native Rust parses/decrypts; invite locator is opaque; Windows per-user URI and clipboard formats; ProjectInvite backend settings remain signed |
| QUIC/TCP/Noise/Yamux and behavior configuration | `crates/network/src/behaviour.rs` | Config, codec, discovery |
| Swarm ownership, request authorization, preferred connections | `crates/network/src/actor.rs` | Discovery and status |
| Allow-list refresh, QUIC/TCP direct retry, learned addresses | `crates/network/src/discovery.rs` | Trusted cards and endpoint records |
| Bounded command/RPC interface | `crates/network/src/handle.rs` | Actor |
| PeerId/card key binding | `crates/network/src/identity.rs` | Crypto signatures |
| Online/route/transport and authorized inbound event observation | `crates/network/src/status.rs`, `actor.rs` | Actor correlates a direct listener connection with an authorized transfer request; reachability stays `UNVERIFIED` without cross-network provenance; no remote IP is serialized |
| Binary chunk and bounded JSON frames | `crates/network/src/codec.rs` | Peer message types |
| Device/project keys, signatures, DPAPI | `crates/crypto/src/lib.rs` | Protocol signing bytes/AAD |
| Scanner, chunk store, safe paths, Windows file/project locks | `crates/snapshot/src/lib.rs` | Manifest model and transfer disk operations |
| Event-triggered inventory quiet period and writer-lock settlement | `crates/snapshot/src/settlement.rs` | Existing guarded scanner; no watcher |
| Manifest/path validation, signing serialization, peer/IPC/invitation types | `shared/protocol/src/lib.rs`, `shared/protocol/src/endpoint.rs` | Endpoint record domain, bounds, time/sequence and IP Multiaddr validation; all crates |
| Signed HTTP request scope, timestamp, nonce and body binding | `shared/protocol/src/supabase.rs`, `crates/crypto/src/supabase.rs` | Supabase wire types; `*_MAILBOX_*` names and AAD strings are stable wire values |

## Public call paths

```text
snapshot: Engine::snapshot → ProjectLock → snapshot::scan → Identity::sign_snapshot → Store::commit_local
check:    Engine::check_peer → fetch_snapshot_chain → validate_snapshot → lineage → peer Verified ACK
receive:  Engine::receive_from_peer → fetch_snapshot_chain → Receiver::receive → download → apply
recover:  Engine::recover → recover_project → trusted target validation → apply
serve:    PeerRuntime → transfer::serve → project authorization → head/manifest/indexed chunk lookup
refresh:  Engine::refresh_metadata → bounded page → trust/signature/AEAD → page + cursor transaction
desktop:  typed Tauri command → bounded tracked job → Engine workflow → compact status page
pairing:  native invite import → existing actor reload/connect → Engine request_pairing → proof-verified pending candidate → explicit approve_pairing → existing pair/trust reload
send:     Engine::send_snapshot → blocking snapshot commit → publish_metadata → ACK or durable PENDING
plugin:   VST3 worker → bounded per-user Named Pipe → PluginRuntime → Engine summary/send_snapshot
save:     SAVE_OBSERVED → project debounce → snapshot_after_save → scan_after_save → commit → publish/ACK
```

`PeerRuntime` owns the network actor/responder lifetime. `Network` is a handle, not another swarm. SQLite ownership remains in `Store`; each domain module adds methods to that same type. Infrastructure services own independent stores/swarms in their separate processes.

`PeerRuntime` holds an exclusive Windows data-directory lease shared with its actor/responder tasks. CLI-only plugin owners use `SessionGrace`: live plugin sessions, accepted local work, and active peer responses retain the owner. Brief requests between polls reset grace through activity epochs; advisory presence refreshes do not. Plugin-launched Desktop owners move into the tray after idle grace and reuse the persisted first-tray notice. Desktop window close retains the user session in the native tray; tray/Open and explicit desktop reopen restore the same window/process. Close safely and tray/Quit confirm collaborator availability loss, reject new IPC under the acceptance gate and drain workers before stopping the one peer runtime. Status summaries use bounded storage queries in `projects.rs`, `snapshots.rs`, and `transfers.rs`; `local_status` retains its complete CLI output.

## Targeted search examples

```powershell
rg -n 'commit_local|begin_apply|finish_apply' crates/storage/src crates/transfer/src
rg -n 'TransferState|requires_recovery' crates/storage/src crates/transfer/src
rg -n 'PROTOCOL_VERSION|signing_bytes|aad' shared/protocol/src crates/crypto/src
rg -n 'send_request|authorized' crates/network/src
```

Exclude `target`, ignored scratch/data folders, and `.git` from broad searches. If parent ignore rules hide source, use `rg --no-ignore` against the explicit `crates`, `shared`, or `docs` path rather than the whole drive.
