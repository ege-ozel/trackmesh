-- Abuse limits for a service whose URL and publishable key travel inside every invite link.
--
-- 1. owner_keys: when this table has any row, only those device public keys may claim a new
--    project route. Invitees never claim, so they never need a row. Empty table = open claims,
--    as before. The operator adds rows in the SQL Editor; no client can read or write it.
-- 2. invite-create: at most 10 invites per project route per hour, checked after the owner check
--    and before the global hourly limit, so one route cannot use the whole global limit.
-- 3. invite-consume: an unknown or expired ID no longer counts against any shared limit, so random
--    IDs cannot block real invites. Real IDs are bounded by invite-create and the per-ID limit.
begin;

create table public.owner_keys (
  public_key text primary key check (public_key ~ '^[0-9a-f]{64}$'),
  note text check (note is null or char_length(note) <= 200),
  created_at timestamptz not null default clock_timestamp()
);
alter table public.owner_keys enable row level security;
revoke all on table public.owner_keys from public, anon, authenticated;

create function public.trackmesh_claim_allowed(p_public_key text)
returns boolean language sql stable security definer set search_path = pg_catalog as $$
  select not exists (select 1 from public.owner_keys)
    or exists (select 1 from public.owner_keys where public_key = p_public_key);
$$;
revoke all on function public.trackmesh_claim_allowed(text) from public, anon, authenticated;

-- Same wrapper as 20261019000100, plus the claim check before any nonce or rate-limit row.
create or replace function public.trackmesh_dispatch(
  p_operation text, p_project_route uuid, p_device_id uuid, p_public_key text,
  p_issued_at timestamptz, p_nonce uuid, p_body_hash text, p_signature text, p_body jsonb
) returns jsonb language plpgsql security definer set search_path = pg_catalog as $$
begin
  if p_operation = 'project-claim'
    and not public.trackmesh_claim_allowed(p_public_key)
  then return jsonb_build_object('accepted',false); end if;
  if p_operation = 'member-grant'
    and not public.trackmesh_is_owner(p_project_route, p_device_id, p_public_key)
  then return jsonb_build_object('accepted',false); end if;
  return public.trackmesh_dispatch_v1(
    p_operation, p_project_route, p_device_id, p_public_key,
    p_issued_at, p_nonce, p_body_hash, p_signature, p_body
  );
end;
$$;

alter table public.invite_rate_limits drop constraint invite_rate_limits_operation_check;
alter table public.invite_rate_limits add constraint invite_rate_limits_operation_check
  check (operation in ('invite-create-global','invite-create-route','invite-consume-global','invite-consume-id'));

create or replace function public.trackmesh_invite_create(
  p_project_route uuid, p_device_id uuid, p_public_key text, p_issued_at timestamptz,
  p_nonce uuid, p_id text, p_expires_at timestamptz, p_ciphertext text
) returns jsonb language plpgsql security definer set search_path = pg_catalog as $$
begin
  -- Owner first, then this route's own limit, then (inside _v1) the global limit.
  if not public.trackmesh_is_owner(p_project_route, p_device_id, p_public_key)
  then return jsonb_build_object('accepted',false); end if;
  if p_project_route is null
    or not public.trackmesh_invite_rate_limit('invite-create-route', p_project_route::text, 10)
  then return jsonb_build_object('accepted',false); end if;
  return public.trackmesh_invite_create_v1(
    p_project_route, p_device_id, p_public_key, p_issued_at,
    p_nonce, p_id, p_expires_at, p_ciphertext
  );
end;
$$;

create or replace function public.trackmesh_invite_consume(p_id text)
returns jsonb language plpgsql security definer set search_path = pg_catalog as $$
declare
  v_drop public.invite_drops%rowtype;
begin
  if p_id is null or p_id !~ '^[A-Za-z0-9_-]{43}$' then
    return jsonb_build_object('accepted',false);
  end if;
  perform public.trackmesh_invite_cleanup_batch();
  -- A miss writes nothing: guessing a 256-bit ID is hopeless, and counting misses in a shared
  -- bucket would let anyone block real invites.
  if not exists (
    select 1 from public.invite_drops where id=p_id and expires_at>clock_timestamp()
  ) then return jsonb_build_object('accepted',false); end if;
  if not public.trackmesh_invite_rate_limit('invite-consume-id',p_id,10)
  then return jsonb_build_object('accepted',false); end if;
  delete from public.invite_drops
    where id=p_id and expires_at>clock_timestamp()
    returning * into v_drop;
  if not found then return jsonb_build_object('accepted',false); end if;
  return jsonb_build_object('accepted',true,'ciphertext',v_drop.ciphertext,
    'expires_at',to_char(v_drop.expires_at at time zone 'UTC','YYYY-MM-DD"T"HH24:MI:SS.MS"Z"'));
end;
$$;

-- create or replace keeps existing grants; restate them so this file alone shows the ACL.
revoke all on function public.trackmesh_dispatch(text,uuid,uuid,text,timestamptz,uuid,text,text,jsonb) from public, anon, authenticated;
grant execute on function public.trackmesh_dispatch(text,uuid,uuid,text,timestamptz,uuid,text,text,jsonb) to service_role;
revoke all on function public.trackmesh_invite_create(uuid,uuid,text,timestamptz,uuid,text,timestamptz,text) from public, anon, authenticated;
grant execute on function public.trackmesh_invite_create(uuid,uuid,text,timestamptz,uuid,text,timestamptz,text) to service_role;
revoke all on function public.trackmesh_invite_consume(text) from public, anon, authenticated;
grant execute on function public.trackmesh_invite_consume(text) to service_role;

commit;
