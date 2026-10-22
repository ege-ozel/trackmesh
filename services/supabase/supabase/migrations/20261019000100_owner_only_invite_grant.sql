-- Only the project owner (the device that claimed the route) creates invite links and grants
-- membership. The existing functions are kept under a _v1 name and are reachable only through
-- these wrappers, which reject a non-owner before any nonce, sequence or rate-limit row changes.
begin;

alter function public.trackmesh_dispatch(text,uuid,uuid,text,timestamptz,uuid,text,text,jsonb)
  rename to trackmesh_dispatch_v1;
revoke all on function public.trackmesh_dispatch_v1(text,uuid,uuid,text,timestamptz,uuid,text,text,jsonb)
  from public, anon, authenticated, service_role;

alter function public.trackmesh_invite_create(uuid,uuid,text,timestamptz,uuid,text,timestamptz,text)
  rename to trackmesh_invite_create_v1;
revoke all on function public.trackmesh_invite_create_v1(uuid,uuid,text,timestamptz,uuid,text,timestamptz,text)
  from public, anon, authenticated, service_role;

create function public.trackmesh_is_owner(p_project_route uuid, p_device_id uuid, p_public_key text)
returns boolean language sql stable security definer set search_path = pg_catalog as $$
  select exists (
    select 1 from public.project_members
    where project_route = p_project_route and device_id = p_device_id
      and public_key = p_public_key and role = 'owner' and status = 'active'
  );
$$;
revoke all on function public.trackmesh_is_owner(uuid,uuid,text) from public, anon, authenticated;

create function public.trackmesh_dispatch(
  p_operation text, p_project_route uuid, p_device_id uuid, p_public_key text,
  p_issued_at timestamptz, p_nonce uuid, p_body_hash text, p_signature text, p_body jsonb
) returns jsonb language plpgsql security definer set search_path = pg_catalog as $$
begin
  if p_operation = 'member-grant'
    and not public.trackmesh_is_owner(p_project_route, p_device_id, p_public_key)
  then return jsonb_build_object('accepted',false); end if;
  return public.trackmesh_dispatch_v1(
    p_operation, p_project_route, p_device_id, p_public_key,
    p_issued_at, p_nonce, p_body_hash, p_signature, p_body
  );
end;
$$;
revoke all on function public.trackmesh_dispatch(text,uuid,uuid,text,timestamptz,uuid,text,text,jsonb) from public, anon, authenticated;
grant execute on function public.trackmesh_dispatch(text,uuid,uuid,text,timestamptz,uuid,text,text,jsonb) to service_role;

create function public.trackmesh_invite_create(
  p_project_route uuid, p_device_id uuid, p_public_key text, p_issued_at timestamptz,
  p_nonce uuid, p_id text, p_expires_at timestamptz, p_ciphertext text
) returns jsonb language plpgsql security definer set search_path = pg_catalog as $$
begin
  -- Checked before the global hourly limit, so non-owners cannot use it up.
  if not public.trackmesh_is_owner(p_project_route, p_device_id, p_public_key)
  then return jsonb_build_object('accepted',false); end if;
  return public.trackmesh_invite_create_v1(
    p_project_route, p_device_id, p_public_key, p_issued_at,
    p_nonce, p_id, p_expires_at, p_ciphertext
  );
end;
$$;
revoke all on function public.trackmesh_invite_create(uuid,uuid,text,timestamptz,uuid,text,timestamptz,text) from public, anon, authenticated;
grant execute on function public.trackmesh_invite_create(uuid,uuid,text,timestamptz,uuid,text,timestamptz,text) to service_role;

commit;
