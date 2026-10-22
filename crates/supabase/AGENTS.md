# Supabase client changes

Read root `AGENTS.md`. This crate makes bounded HTTPS requests to the optional per-user Supabase service; core owns trust and project decisions and storage owns durable commits.

- Require HTTPS to a hosted `*.supabase.co` origin with certificate verification. Never follow redirects or accept invalid certificates.
- Send only signed requests and encrypted invite drops, endpoint records and head hints. Never send manifests, files, chunks, invitation payloads or project keys in the clear.
- Preserve signing bytes and domains in `shared/protocol/src/`. Bound response bytes, request lifetime and retries.
- Do not log request bodies or auth headers. No automated tests/benchmarks.
