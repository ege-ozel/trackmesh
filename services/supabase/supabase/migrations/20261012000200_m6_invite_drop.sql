begin;

create table public.invite_drops (
  id text primary key check (id ~ '^[A-Za-z0-9_-]{43}$'),
  ciphertext text not null check (octet_length(ciphertext) < 65536 and ciphertext ~ '^[A-Za-z0-9_-]+$'),
  expires_at timestamptz not null,
  created_at timestamptz not null default clock_timestamp()
);
create index invite_drops_expiry on public.invite_drops (expires_at);

create table public.invite_request_nonces (
  project_route uuid not null,
  device_id uuid not null,
  nonce uuid not null,
  created_at timestamptz not null default clock_timestamp(),
  primary key (project_route, device_id, nonce)
);
create index invite_request_nonces_created_at on public.invite_request_nonces(created_at);

create table public.invite_rate_limits (
  operation text not null check (operation in ('invite-create-global','invite-consume-global','invite-consume-id')),
  subject_id text not null,
  bucket_start timestamptz not null,
  request_count integer not null check (request_count > 0),
  primary key (operation, subject_id, bucket_start)
);
create index invite_rate_limits_bucket on public.invite_rate_limits(bucket_start);

alter table public.invite_drops enable row level security;
alter table public.invite_request_nonces enable row level security;
alter table public.invite_rate_limits enable row level security;
revoke all on table public.invite_drops from public, anon, authenticated;
revoke all on table public.invite_request_nonces from public, anon, authenticated;
revoke all on table public.invite_rate_limits from public, anon, authenticated;

create function public.trackmesh_invite_rate_limit(
  p_operation text, p_subject_id text, p_limit integer
) returns boolean language plpgsql security definer set search_path = pg_catalog as $$
declare
  v_bucket timestamptz;
  v_count integer;
begin
  v_bucket := to_timestamp(floor(extract(epoch from clock_timestamp()) / 3600) * 3600);
  insert into public.invite_rate_limits(operation,subject_id,bucket_start,request_count)
    values (p_operation,p_subject_id,v_bucket,1)
  on conflict (operation,subject_id,bucket_start)
    do update set request_count=public.invite_rate_limits.request_count+1
  returning request_count into v_count;
  return v_count <= p_limit;
end;
$$;

create function public.trackmesh_invite_cleanup_batch()
returns void language plpgsql security definer set search_path = pg_catalog as $$
begin
  delete from public.invite_drops where id in (
    select id from public.invite_drops where expires_at <= clock_timestamp() order by expires_at limit 500
  );
  delete from public.invite_request_nonces where (project_route,device_id,nonce) in (
    select project_route,device_id,nonce from public.invite_request_nonces
    where created_at <= clock_timestamp()-interval '10 minutes' order by created_at limit 1000
  );
  delete from public.invite_rate_limits where (operation,subject_id,bucket_start) in (
    select operation,subject_id,bucket_start from public.invite_rate_limits
    where bucket_start <= clock_timestamp()-interval '48 hours' order by bucket_start limit 1000
  );
end;
$$;

create function public.trackmesh_invite_create(
  p_project_route uuid, p_device_id uuid, p_public_key text, p_issued_at timestamptz,
  p_nonce uuid, p_id text, p_expires_at timestamptz, p_ciphertext text
) returns jsonb language plpgsql security definer set search_path = pg_catalog as $$
declare
  v_member public.project_members%rowtype;
begin
  if p_project_route is null or p_project_route='00000000-0000-0000-0000-000000000000'::uuid
    or p_device_id is null or p_device_id='00000000-0000-0000-0000-000000000000'::uuid
    or p_public_key is null or p_public_key !~ '^[0-9a-f]{64}$'
    or p_issued_at is null or p_issued_at < clock_timestamp()-interval '2 minutes'
    or p_issued_at > clock_timestamp()+interval '2 minutes'
    or p_nonce is null or p_nonce='00000000-0000-0000-0000-000000000000'::uuid
    or p_id is null or p_id !~ '^[A-Za-z0-9_-]{43}$'
    or p_expires_at is null or p_expires_at <= clock_timestamp()
    or p_expires_at > clock_timestamp()+interval '15 minutes'
    or p_ciphertext is null or p_ciphertext !~ '^[A-Za-z0-9_-]+$'
    or octet_length(p_ciphertext) >= 65536
  then raise exception using errcode='P0001', message='request rejected'; end if;

  perform pg_advisory_xact_lock(hashtextextended(p_project_route::text,847211));
  perform public.trackmesh_invite_cleanup_batch();
  if not public.trackmesh_invite_rate_limit('invite-create-global','all-projects',60) then
    return jsonb_build_object('accepted',false);
  end if;
  select * into v_member from public.project_members
    where project_route=p_project_route and device_id=p_device_id for update;
  if not found or v_member.public_key<>p_public_key or v_member.status<>'active' then
    return jsonb_build_object('accepted',false);
  end if;
  insert into public.invite_request_nonces(project_route,device_id,nonce)
    values (p_project_route,p_device_id,p_nonce) on conflict do nothing;
  if not found then return jsonb_build_object('accepted',false); end if;
  insert into public.invite_drops(id,ciphertext,expires_at)
    values(p_id,p_ciphertext,p_expires_at) on conflict do nothing;
  if not found then return jsonb_build_object('accepted',false); end if;
  return jsonb_build_object('accepted',true);
end;
$$;

create function public.trackmesh_invite_consume(p_id text)
returns jsonb language plpgsql security definer set search_path = pg_catalog as $$
declare
  v_drop public.invite_drops%rowtype;
begin
  if p_id is null or p_id !~ '^[A-Za-z0-9_-]{43}$' then
    return jsonb_build_object('accepted',false);
  end if;
  perform public.trackmesh_invite_cleanup_batch();
  if not public.trackmesh_invite_rate_limit('invite-consume-global','all-projects',300)
    or not public.trackmesh_invite_rate_limit('invite-consume-id',p_id,10)
  then return jsonb_build_object('accepted',false); end if;
  delete from public.invite_drops
    where id=p_id and expires_at>clock_timestamp()
    returning * into v_drop;
  if not found then return jsonb_build_object('accepted',false); end if;
  return jsonb_build_object('accepted',true,'ciphertext',v_drop.ciphertext,
    'expires_at',to_char(v_drop.expires_at at time zone 'UTC','YYYY-MM-DD"T"HH24:MI:SS.MS"Z"'));
end;
$$;

revoke all on function public.trackmesh_invite_rate_limit(text,text,integer) from public, anon, authenticated;
revoke all on function public.trackmesh_invite_cleanup_batch() from public, anon, authenticated;
revoke all on function public.trackmesh_invite_create(uuid,uuid,text,timestamptz,uuid,text,timestamptz,text) from public, anon, authenticated;
revoke all on function public.trackmesh_invite_consume(text) from public, anon, authenticated;
grant execute on function public.trackmesh_invite_create(uuid,uuid,text,timestamptz,uuid,text,timestamptz,text) to service_role;
grant execute on function public.trackmesh_invite_consume(text) to service_role;

commit;
