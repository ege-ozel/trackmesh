# Mailbox client (not built)

The app side of the self-hosted mailbox in `../mailbox-service/`. It published an encrypted, signed "new version" notice for each snapshot and read the collaborator's notices back. The per-user Supabase head hints and direct notices replaced it. It was removed from the build in commit `d447de1`; the files here are copies from `d447de1~1`, plus the unused protocol types removed later.

Nothing here is compiled. The files may no longer build as they are.

## What each file was

| File | Was | Did |
| --- | --- | --- |
| `core_mailbox.rs` | `crates/core/src/mailbox.rs` | `MailboxRuntime`: durable publish of own snapshots, ACK check, page/cursor refresh, 30-second retry |
| `storage_mailbox.rs` | `crates/storage/src/mailbox.rs` | Pending envelopes, publication ACK, announcement pages and cursor in SQLite |
| `crypto_mailbox.rs` | `crates/crypto/src/mailbox.rs` | Signed short-lived HTTP request auth (`verify_mailbox_auth`) |
| `crypto_seal_open.rs` | part of `crates/crypto/src/lib.rs` | `Identity::seal`, `verify_envelope`, `open`: encrypt/sign and verify/decrypt an `Announcement` |
| `http.rs` | `crates/mailbox/src/http.rs` | `MailboxClient`: bounded HTTPS POST/GET, URL checks |
| `protocol_mailbox.rs` | part of `shared/protocol/src/lib.rs` and `mailbox.rs` | `Announcement`, `Envelope`, `MailboxRecord/Page/Ack`, `MailboxAuth`, `mailbox_scope`, size limits |
| `AGENTS.md` | `crates/mailbox/AGENTS.md` | Rules for the HTTP client crate |

## Still in the product, for compatibility

- `ProjectInvite.mailbox_url` in the signed invitation: written empty, ignored when read.
- SQLite `projects.mailbox_url` column and `mailbox_state` table: kept, not read. `link` clears `mailbox_url`.
- `SELF_HOSTED_MAILBOX` backend value in settings: still parsed, behaves like `NONE`.
- `pending_publications` in the plugin status: always 0.
- The `PENDING`/`PUBLISHED` snapshot metadata values.

## To bring it back

1. Put `protocol_mailbox.rs` back into `shared/protocol` and re-export its types. Keep the strings `trackmesh-mailbox-v1` and `trackmesh-mailbox-request-v1` and the limits unchanged.
2. Recreate a crate (it was `crates/mailbox`, package `trackmesh-mailbox`) with `http.rs`; dependencies were `anyhow`, `base64`, `crypto`, `protocol`, `reqwest`, `rustls`, `serde`, `serde_json`, `uuid`. Add it to the workspace.
3. Add `crypto_mailbox.rs` as `mod mailbox` in `crates/crypto` and the `crypto_seal_open.rs` functions back into `crates/crypto/src/lib.rs` (it needs `chacha20poly1305` and `base64` again).
4. Add `storage_mailbox.rs` as `mod mailbox` in `crates/storage`; the schema still has its table.
5. Add `core_mailbox.rs` to `crates/core`, start `MailboxRuntime` from the CLI/Desktop owners, and wire the share result and lifetime (pending publication keeps the owner alive).
6. Read and write `mailbox_url` again in `link`, invitations and settings.
7. Add a bounded retry lifetime first; see `docs/plans/post-1.0.md`.
