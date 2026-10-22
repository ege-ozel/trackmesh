# Core and CLI changes

Read root `AGENTS.md`. Core owns reusable application decisions; adapters own presentation and their session lifetime.

- `src/lib.rs`: engine/data ownership, project linking, pairing/invitations, private namespaces.
- `src/snapshots.rs`: manual/settled snapshot creation shares the project lock, recovery guard, unchanged-content reuse and transactional commit. `src/sending.rs` publishes only after this commit; settlement runs in blocking workers.
- `src/workflows.rs`: typed local status, online peer check, receive setup, explicit recovery. Future UI/IPC should call these methods.
- `src/status.rs`: compact bounded project/peer/transfer summaries for adapters; full CLI status remains unchanged.
- `src/sending.rs`: explicit manual snapshot and its result.
- `src/pairing.rs`: ephemeral pairing windows, one-use invitation possession challenge, bounded pending candidates, explicit approval/rejection and automatic reply/approval workflow. Keep all project decisions in core; only approval calls existing `pair` to grant DB authorization. No project keys or proof payloads enter React/logs.
- `src/peer.rs`: network/responder lifetime, blocking request workers, signed ancestry retrieval.
- Peer runtime startup acquires an exclusive Windows data-directory lease held through actor/responder drain, including when an adapter handle is dropped. Adapters must explicitly stop their session.
- `src/supabase.rs`: long-lived signed endpoint-mailbox claim/grant/poll session, bounded cursor refresh and retry; every received record uses `Engine::apply_endpoint_record`. `src/settings.rs` stores project backend settings and durable grant sequence/queue outside SQLite with fail-closed version handling.
- Metadata methods share a per-project coordinator within an Engine so foreground commands and the session worker cannot race page cursors; SQLite expected-cursor guards remain authoritative across processes.
- `src/main.rs`: CLI parsing, bounded input files, output, network startup/wait/stop, Ctrl+C. Keep lineage and snapshot-install decisions in core/transfer.
- `src/ipc`: current-user-only Windows pipe, bound plugin sessions, compact status and bounded project jobs in `jobs.rs`. Coalesce save observations by project; observations during active work cause a settled follow-up pass. Accepted jobs outlive disconnect/rebind. Reuse the owner's Engine/network; receive is a DAW-closed desktop handoff. Idle close must claim the same gate as connection/job acceptance; `stop` awaits accepted work including debounce.
- `src/lifetime.rs`: shared session/work/publication retention and 60-second grace. Brief plugin/peer activity resets grace through epochs. Pending detection is an authored-local SQLite EXISTS query in a blocking worker; no watcher or full history scan. Adapter jobs/window sessions also retain desktop; stop metadata/peer only after local work drains.

Maintain separate project/data directories, persistent DPAPI identity, immutable manifests, and project locks. Snapshot creation rejects pending recovery. Every ancestor is checked for project/requested ID, trusted author, signature/hash, cycle, and bounded history depth.

Online equality needs the contacted peer's acknowledgement. Local status reports last-known verification; it must not imply an offline source is downloadable or currently equal. Network shutdown must drain responders; blocking file apply must retain its lock and finish safely after Ctrl+C.

Core snapshot/recovery methods perform disk work. Desktop/IPC callers must schedule them away from UI and DAW audio threads. Work-based owner retention is implemented; do not install a permanent Windows daemon.

Preserve CLI flags and JSON fields during structural refactors. The local Windows CLI/desktop/VST workflow includes save settlement, session lifetime, bounded remote history, offline availability and receive handoff. Real DAW and Internet deployment integrations remain unverified. No automated tests/benchmarks.
