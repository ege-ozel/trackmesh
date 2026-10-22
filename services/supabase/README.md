# TrackMesh Supabase backend (M5d)

This directory contains the optional Supabase invite service: encrypted single-read invite drops, signed encrypted endpoint records, encrypted head hints, and device membership. The client implementation lives in `crates/mailbox`, `crates/crypto`, `crates/core`, and the Windows Desktop adapter. [IMPLEMENTATION.md](../../docs/IMPLEMENTATION.md) records a live host/Windows Sandbox invite and transfer trial. The m7 migration reached pushed `master` on 2026-10-13, but its application has not been checked in the Dashboard; `head-publish` and `head-list` deployment remains unconfirmed. Project UUIDs are opaque routes, not project content. Plaintext addresses, project keys, invite contents, files and samples never go to Supabase.

Projects that use this service do not use the legacy self-hosted mailbox: Share publishes no mailbox metadata, and offline collaborators learn about new versions from the encrypted head hints.

## Before deployment

- Review the owner decisions and signing contract in [`docs/plans/phase1-supabase-backend.md`](../../docs/plans/phase1-supabase-backend.md), especially TOFU `project-claim`, member grants/revocations, and the exact tuple bytes.
- Use a dedicated Supabase project per environment and keep its owner account protected with MFA.
- TrackMesh clients use the Project URL and a new-format publishable key (`sb_publishable_...`) only. These identify the project and do not grant membership. Do not use legacy JWT anon keys.
- Never give a secret key (`sb_secret_...`) or database password to a TrackMesh user, client, repository, support ticket, or log. Edge Functions read the server-only `SUPABASE_SECRET_KEYS` JSON environment variable and use its `default` secret key. It must never be returned or logged.
- Supabase secret keys map to the `service_role` database role; keep the migration's `GRANT EXECUTE ... TO service_role` statements unchanged.
- The Functions disable gateway JWT verification because TrackMesh authenticates requests with its own Ed25519 signature. Signed routes must pass that verification and their SQL ACL/nonce checks. The intentionally unsigned invite-consume route uses the drop locator, TTL, one-read and rate-limit checks. A public API key or function URL is not authorization.

## Supabase Dashboard setup

1. Create a Supabase project and choose its region and plan. In **Project Settings → API Keys**, copy the Project URL and the new-format `sb_publishable_...` key. The publishable key is public; membership still depends on TrackMesh device signatures and server ACL checks. Never distribute the `sb_secret_...` key or database password.
2. If the GitHub integration is not configured, apply in **SQL Editor** [`supabase/migrations/20261012000100_trackmesh_m5d.sql`](supabase/migrations/20261012000100_trackmesh_m5d.sql) once. Check that the four tables have RLS enabled, no `anon`/`authenticated` policies or grants, and that the RPC is executable only by `service_role`.
3. Without the integration, apply [`supabase/migrations/20261012000200_m6_invite_drop.sql`](supabase/migrations/20261012000200_m6_invite_drop.sql) once after the M5d migration. Check that `invite_drops`, `invite_request_nonces`, and `invite_rate_limits` have RLS enabled and no `anon`/`authenticated` policies or grants.
4. Apply [`supabase/migrations/20261013000100_m7_head_records.sql`](supabase/migrations/20261013000100_m7_head_records.sql) after M5d. This additive migration creates encrypted head metadata storage and extends `trackmesh_dispatch`; it does not change prior migrations.
5. In **Edge Functions**, deploy `project-claim`, `member-grant`, `member-revoke`, `endpoint-publish`, `endpoint-list`, `head-publish`, `head-list`, `invite-create`, and `invite-consume`. Use the function source bundle so `supabase/functions/_shared/http.ts` is included. Keep JWT verification disabled for all nine functions; the committed `supabase/config.toml` records that setting.
6. Do not set any secret by hand. Supabase injects `SUPABASE_URL` and `SUPABASE_SECRET_KEYS` (a JSON dictionary whose `default` entry is the project secret key) into every Edge Function. Edge Functions fail closed with a generic server error if the value is absent or invalid. The secret remains server-side; no Function sends an `Authorization: Bearer` header.
7. Keep the client URL/key separate from the Function secret. A deployment without the injected privileged key must fail closed; do not work around it by putting a secret key in the desktop application.

The Dashboard editor is useful for an initial project and inspection. The repository remains the source of truth for migration and Function source; Dashboard edits do not update this checkout. Prefer the CLI path below for repeatable deployments.

## Supabase CLI setup

Run from this directory with the official Supabase CLI installed and authenticated. The project ref and database password are deployment inputs; do not store a database password in a tracked file.

```powershell
supabase login
supabase link --project-ref <project-ref>
supabase db push
supabase functions deploy project-claim --no-verify-jwt
supabase functions deploy member-grant --no-verify-jwt
supabase functions deploy member-revoke --no-verify-jwt
supabase functions deploy endpoint-publish --no-verify-jwt
supabase functions deploy endpoint-list --no-verify-jwt
supabase functions deploy head-publish --no-verify-jwt
supabase functions deploy head-list --no-verify-jwt
supabase functions deploy invite-create --no-verify-jwt
supabase functions deploy invite-consume --no-verify-jwt
```

`supabase db push` applies committed migrations to the linked project. Review the target ref before running it. Edge Functions use `SUPABASE_URL` and `SUPABASE_SECRET_KEYS`; this package does not define or track secret values. The Supabase GitHub integration is configured with working directory `services/supabase` and branch `master`; a push to `master` applies migrations automatically. Once a migration has been deployed, do not edit that migration file: make corrections in a new migration. Deploy Functions with the `supabase functions deploy <function-name> --no-verify-jwt` commands above if the integration does not publish them. Local `supabase start` / `supabase functions serve` can be used for development, but never paste local service keys into repository files or agent/chat logs. `supabase/config.toml` contains no project ref, URL, API key, or secret.

TrackMesh's **Default Supabase service** settings save the Project URL and publishable key in the app's global `settings.json`; this prefills project backend settings. A v2 invite link carries its service reference and publishable key, so a recipient can consume it without entering service details. Private builds may embed defaults with `TRACKMESH_DEFAULT_SUPABASE_URL` and `TRACKMESH_DEFAULT_SUPABASE_PUBLISHABLE_KEY`, either as environment variables or in a local `[env]` table in the gitignored `.cargo/config.toml`. Public releases must not embed a personal Supabase project. Never put actual project values in the repository or documents.

## API contract and limits

The five M5d routes accept POST and the `x-trackmesh-auth` header. `invite-create` uses the same signed header and scope; `invite-consume` is intentionally unsigned because a recipient has not joined the project yet. The body hash is SHA-256 over exact request bytes. The signed tuple uses `trackmesh-supabase-request-v1`; timestamps use UTC `YYYY-MM-DDTHH:mm:ss.SSSZ` and nonce is lowercase canonical UUIDv4. Authenticated routes check scope, Ed25519 signature, ±2 minute timestamp window, nonce, bounded body, and strict JSON fields.

M5d state changes happen in one `trackmesh_dispatch` Postgres function invocation. M6 uploads call `trackmesh_invite_create`, which verifies the active signer membership and consumes its nonce in one transaction. `invite-consume` calls `trackmesh_invite_consume`; a conditional `DELETE ... RETURNING` consumes and removes a drop atomically so concurrent recipients cannot both obtain it. The `invite_drops` table has no project route or invite plaintext. All three invite tables and their RPCs are inaccessible to `anon`/`authenticated`; RLS is deny-all. Errors are generic. Functions do not log headers, bodies, ciphertext, public keys, signatures, API keys, or SQL errors.

Invite IDs are 256 random bits encoded base64url. The link fragment contains the separate 256-bit decryption key. Ciphertext stays under 64 KiB and expires within 15 minutes. `invite-create` requires a valid active project member signature. `invite-consume` accepts only an ID because the recipient is not yet a member; this allows a person who learns the locator to consume it and deny the intended recipient. Only the route owner can create invites: each route at most 10/hour, and all routes together 60/hour. Each existing ID allows 10 consume attempts/hour; an unknown or expired ID returns a generic refusal and writes nothing, so random IDs cannot use up a shared limit (migration `20261021000100`). No IP value is trusted or stored. Bounded cleanup runs in the RPC transaction on calls and removes at most 500 expired drops, 1,000 old nonces, and 1,000 old rate buckets.

Initial fixed-window limits are:

| Operation | Per device | Per project |
| --- | ---: | ---: |
| `project-claim` | 3/hour | 10/hour |
| `member-grant` / `member-revoke` | 10/hour | 30/hour |
| `endpoint-publish` / `head-publish` | 30/minute | 120/minute |
| `endpoint-list` / `head-list` | 60/minute | 180/minute |
| `invite-create` | owner only, 60/hour global | 10/hour per route |
| `invite-consume` | 10/hour per existing ID | unknown IDs are not counted |

`project-claim` additionally has one global limit of 60/hour across all routes in this Supabase project. We do not use per-IP limiting: Supabase documents request header and log field names, but the documents inspected do not establish that the Edge Function receives a spoof-resistant client-IP header. The source remains `[V]`; global claim throttling limits random-route/key churn meanwhile.

The global limit is shared by every user of the Supabase project. Without an owner list, anyone who has seen one invite link (it carries the URL and publishable key) can claim fresh random routes, become their owner and use the global claim and invite limits until the hourly window resets. Per-IP limiting remains `[V]` until a trusted client-IP source is established.

### Owner list (recommended)

`public.owner_keys` decides who may claim a new route. While it is empty, any device may claim, as before. Once it has a row, only those device public keys may claim; the check runs before any nonce or rate-limit row, so refused claims use up nothing. Invite recipients never claim, so only the devices that create projects need a row — normally the PCs of whoever runs this service. Each PC shows its key in Desktop under **Advanced → This device → Public key** (not a secret). Add it once in the **SQL Editor**:

```sql
insert into public.owner_keys (public_key, note) values ('<64 lowercase hex characters>', 'studio PC');
```

A reinstalled TrackMesh with a new identity, or a new PC, needs its own row. A PC not on the list sees "The invite service won't let this PC create invites" on its project card; direct sharing with already paired collaborators keeps working. Remove a row with `delete from public.owner_keys where public_key = '…';` — routes that PC already owns keep their owner.

The Rust client stores endpoint data as an XChaCha20-Poly1305 wrapper encrypted with the project key and AAD `trackmesh-endpoint-mailbox-v1`. Its JSON shape is `{version, project_route, writer_device_id, seq, expires_at, nonce, ciphertext}`. `seq` is a canonical decimal string; `expires_at` is UTC RFC3339 with exactly millisecond precision; `nonce` is 24 random bytes encoded base64url without padding; `ciphertext` is the encrypted serialized signed `EndpointRecord`, also base64url without padding. The client base64url-encodes the complete wrapper JSON into the `ciphertext` field sent to `endpoint-publish`. The outer `seq` and `expires_at` request fields must exactly match the wrapper and inner record. On receipt, the client checks all duplicated metadata before calling the shared trusted endpoint validation path.

These are abuse-control starting limits, not a Supabase platform quota. Failed/expired requests are generic. The cleanup batch removes at most 500 expired endpoint rows, 1,000 old nonces, and 1,000 old rate buckets per accepted operation. It runs inside the RPC transaction; repeated activity drains backlog. There is no `pg_cron` dependency.

`head_records` stores at most one encrypted latest-head record for each project member device. It contains no file data, manifest file list, or chunks. Revoking a member removes that writer's head row.

## Current boundaries and remaining validation

- `project-claim` is trust-on-first-use. A first claimant of a project UUID becomes owner. If another device claims the route first, the real device gets the same generic failure; continue with direct peer/manual invite-file setup. There is no remote reset/recovery operation.
- The device that creates a project claims its Supabase route. Projects imported by #42 or later are marked before backend setup and never call `project-claim`. Projects imported by older builds have no origin marker and retain their prior behavior; the client does not guess their origin.
- Clients call the `member-revoke` route with a signed request to revoke membership. Any active member may revoke itself; only the owner can revoke another device. Grants and revocations are retried in their per-actor sequence order; a later action must not overtake an earlier action. At most three project devices may be active. If the owner revokes itself, no owner remains and the route cannot be claimed again. Owner recovery/transfer is not implemented.
- Revoking a device removes its future access to Supabase metadata, including its published head. It does not rotate the project key or recall project files the device already received; that device retains its existing local files and key. Revocation is not retroactive confidentiality.
- The recorded live host/Sandbox trial covered membership lookup, invite consumption, approval and transfer. It did not validate revocation, three-device limits, second consumption, expiry, clipboard cleanup or internet NAT traversal. Dashboard RLS/grant inspection, rate limits, cleanup and full WebCrypto/Rust equivalence remain unconfirmed. The newer head functions and #42 changes have not had a live check; #42 exposes missing function names in Advanced. Manual invitation files remain an Advanced path when the invite service is unavailable.

No tests or benchmarks are added or run by this package.
