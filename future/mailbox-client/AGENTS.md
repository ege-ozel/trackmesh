# Mailbox client changes

Read root `AGENTS.md`. This crate handles bounded HTTP transport and request authentication; core owns publication/trust decisions and storage owns durable commits.

- Require HTTPS with certificate verification. Plain HTTP is an explicit development option restricted to literal loopback IPs. Never follow redirects or accept invalid certificates.
- Send only existing signed encrypted `Envelope` values, never manifests, files, chunks, invitations, or project keys.
- Preserve envelope signing bytes/AAD. Request authentication has its own domain in `shared/protocol/src/mailbox.rs`.
- Bound response bytes, page count, cursor order, request lifetime, and retries. HTTP success alone is not a publication ACK.
- Do not log request bodies or auth headers. No automated tests/benchmarks.
