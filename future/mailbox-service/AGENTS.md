# Metadata service

Read root `AGENTS.md`. This service stores bounded signed ciphertext, never project keys, manifests, or chunks. Its SQLite database is separate from the agent database.

- `src/main.rs`: loopback HTTP listener, request limits, graceful shutdown. Production HTTPS terminates at the documented reverse proxy.
- `src/api.rs`: bounded requests, signatures, explicit project/device ACL, generic errors.
- `src/store.rs`: persistent nonce/rate limits, immutable envelope insertion with ACK, ordered pages.

ACL documents contain signed public device cards only. No public registration, trust on first use, or project discovery. A missing/invalid ACL fails closed. Preserve existing envelope signing/AAD. No automated tests or benchmarks.
