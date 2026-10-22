# Transfer changes

Read root `AGENTS.md` first. Use `src/lib.rs` for API exports and `../../docs/CODEMAP.md` to choose the operation.

## Owners

- `receiver.rs`: trust/lineage decisions, project lock, download/apply orchestration, failure states, equality ACK.
- `download.rs`: distinct chunks, cache rehash, bounded concurrency/retries, durable chunk progress.
- `verification.rs` and `serving.rs`: author/project validation, working-folder preflight, per-request authorization.
- `assemble.rs`: verified staged file assembly and fsync.
- `apply.rs`: durable phase ordering; `installation.rs`: actual backup/install/final scan and local commit.
- `journal.rs`: durable format/persistence; `recovery.rs`: explicit local recovery entry point.

## Preserve these invariants

1. Validate target signature, project/snapshot binding, and trusted author before acting on peer data.
2. Hold `ProjectLock` across the entire operation and retain it inside blocking apply tasks after cancellation.
3. Reject unsnapshotted changes, divergent/local-ahead/unknown ancestry, and unexpected destination files.
4. Rehash cached chunks on every resume; persist verified chunk data before its completion marker.
5. Assemble/fsync outside the working folder. Never publish a partial file by destination rename.
6. Persist the journal, then set SQLite recovery-required + `APPLYING`, before moving originals.
7. Keep `BACKUP → INSTALL → COMMIT → DONE`. During resumed INSTALL, verify already moved files; do not reassemble them.
8. Verify/fsync files and final folder, then commit history. Retain backups and remove only empty directories.
9. Preserve journal fields/path names and `storage::TransferState` labels. Schema and wire formats are compatibility contracts.

For apply changes inspect `storage/src/transfers.rs` and `storage/src/snapshots.rs` too. Runtime preconditions include a closed DAW. Historical manual recovery evidence is in `docs/IMPLEMENTATION.md`; do not claim it revalidates a later edit. No test/benchmark suites.
