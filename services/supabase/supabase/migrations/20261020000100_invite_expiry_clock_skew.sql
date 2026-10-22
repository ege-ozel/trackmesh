-- The app sets an invite's expiry to its own clock + 15 minutes. A PC clock even slightly ahead
-- of the server made that look longer than 15 minutes and every invite was rejected. Allow the
-- same 2-minute clock difference that the signed request time already allows.
begin;

create or replace function public.trackmesh_invite_create_v1(
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
    or p_expires_at > clock_timestamp()+interval '17 minutes'
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
revoke all on function public.trackmesh_invite_create_v1(uuid,uuid,text,timestamptz,uuid,text,timestamptz,text)
  from public, anon, authenticated, service_role;

commit;
