begin;
create table public.head_records (
 project_route uuid not null, writer_device_id uuid not null,
 seq numeric(20,0) not null check (seq between 1 and 18446744073709551615),
 ciphertext text not null check (length(ciphertext) <= 4096 and ciphertext ~ '^[A-Za-z0-9_-]+$'),
 updated_at timestamptz not null default clock_timestamp(),
 primary key(project_route,writer_device_id),
 foreign key(project_route,writer_device_id) references public.project_members(project_route,device_id)
);
alter table public.head_records enable row level security;
revoke all on table public.head_records from public, anon, authenticated;
alter table public.request_nonces drop constraint request_nonces_operation_check;
alter table public.request_nonces add constraint request_nonces_operation_check check (operation in ('project-claim','member-grant','member-revoke','endpoint-publish','endpoint-list','head-publish','head-list'));
alter table public.rate_limits drop constraint rate_limits_operation_check;
alter table public.rate_limits add constraint rate_limits_operation_check check (operation in ('project-claim','member-grant','member-revoke','endpoint-publish','endpoint-list','head-publish','head-list'));
create or replace function public.trackmesh_dispatch(
  p_operation text, p_project_route uuid, p_device_id uuid, p_public_key text,
  p_issued_at timestamptz, p_nonce uuid, p_body_hash text, p_signature text, p_body jsonb
) returns jsonb language plpgsql security definer set search_path = pg_catalog as $$
declare
  v_member public.project_members%rowtype;
  v_target_id uuid;
  v_target_key text;
  v_actor_seq numeric(20,0);
  v_record_seq numeric(20,0);
  v_expiry timestamptz;
  v_ciphertext text;
  v_cursor bigint;
  v_rows jsonb;
  v_window interval;
  v_device_limit integer;
  v_active_count integer;
begin
  if p_operation is null or p_operation not in ('project-claim','member-grant','member-revoke','endpoint-publish','endpoint-list','head-publish','head-list')
    or p_project_route is null or p_project_route = '00000000-0000-0000-0000-000000000000'::uuid
    or p_device_id is null or p_device_id = '00000000-0000-0000-0000-000000000000'::uuid
    or p_public_key is null or p_public_key !~ '^[0-9a-f]{64}$'
    or p_issued_at is null or p_issued_at < clock_timestamp() - interval '2 minutes'
    or p_issued_at > clock_timestamp() + interval '2 minutes'
    or p_nonce is null or p_nonce = '00000000-0000-0000-0000-000000000000'::uuid
    or p_body_hash is null or p_body_hash !~ '^[0-9a-f]{64}$'
    or p_signature is null or p_signature !~ '^[0-9a-f]{128}$'
    or p_body is null or jsonb_typeof(p_body) <> 'object'
  then raise exception using errcode = 'P0001', message = 'request rejected'; end if;

  perform pg_advisory_xact_lock(hashtextextended(p_project_route::text,847210));
  perform public.trackmesh_cleanup_batch();

  if p_operation = 'project-claim' then
    v_window := interval '1 hour'; v_device_limit := 3;
  elsif p_operation in ('member-grant','member-revoke') then
    v_window := interval '1 hour'; v_device_limit := 10;
  elsif p_operation in ('endpoint-publish','head-publish') then
    v_window := interval '1 minute'; v_device_limit := 30;
  else
    v_window := interval '1 minute'; v_device_limit := 60;
  end if;

  if p_operation <> 'project-claim' then
    select * into v_member from public.project_members
    where project_route = p_project_route and device_id = p_device_id for update;
    if not found or v_member.public_key <> p_public_key then
      raise exception using errcode = 'P0001', message = 'request rejected';
    end if;
  end if;

  perform public.trackmesh_rate_limit(p_project_route,p_operation,'device',p_device_id::text,v_window,v_device_limit);
  perform public.trackmesh_rate_limit(
    p_project_route,p_operation,'project',p_project_route::text,v_window,
    case when p_operation in ('endpoint-publish','head-publish') then 120
         when p_operation in ('endpoint-list','head-list') then 180
         when p_operation = 'project-claim' then 10 else 30 end
  );
  if p_operation = 'project-claim' then
    perform public.trackmesh_rate_limit(
      '00000000-0000-0000-0000-000000000000'::uuid,
      p_operation, 'global', 'all-projects', interval '1 hour', 60
    );
  end if;

  if p_operation <> 'project-claim' and v_member.status <> 'active' then
    return jsonb_build_object('accepted',false);
  end if;

  insert into public.request_nonces(project_route,device_id,nonce,operation,issued_at)
    values (p_project_route,p_device_id,p_nonce,p_operation,p_issued_at) on conflict do nothing;
  if not found then return jsonb_build_object('accepted',false); end if;

  if p_operation = 'project-claim' then
    if (select count(*) from jsonb_object_keys(p_body)) <> 1
      or not (p_body ? 'actor_seq') or p_body->>'actor_seq' <> '1'
    then raise exception using errcode = 'P0001', message = 'request rejected'; end if;
    if exists (select 1 from public.project_members where project_route = p_project_route)
    then return jsonb_build_object('accepted',false); end if;
    insert into public.project_members(
      project_route,device_id,public_key,role,status,granted_by_device_id,grant_seq,grant_signature,granted_at,last_action_seq
    ) values (p_project_route,p_device_id,p_public_key,'owner','active',p_device_id,1,p_signature,p_issued_at,1);
    return jsonb_build_object('accepted',true,'role','owner');
  end if;

  if p_operation in ('member-grant','member-revoke') then
    if jsonb_typeof(p_body->'actor_seq') <> 'string' or p_body->>'actor_seq' !~ '^[1-9][0-9]{0,19}$'
    then raise exception using errcode = 'P0001', message = 'request rejected'; end if;
    v_actor_seq := (p_body->>'actor_seq')::numeric;
    if v_actor_seq > 18446744073709551615 or v_actor_seq <= v_member.last_action_seq
    then return jsonb_build_object('accepted',false); end if;
    update public.project_members set last_action_seq = v_actor_seq
      where project_route = p_project_route and device_id = p_device_id;

    if p_operation = 'member-grant' then
      if (select count(*) from jsonb_object_keys(p_body)) <> 3
        or not (p_body ?& array['actor_seq','target_device_id','target_public_key'])
        or (p_body->>'target_device_id') !~* '^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$'
        or (p_body->>'target_public_key') !~ '^[0-9a-f]{64}$'
      then return jsonb_build_object('accepted',false); end if;
      v_target_id := (p_body->>'target_device_id')::uuid;
      v_target_key := p_body->>'target_public_key';
      if v_target_id = '00000000-0000-0000-0000-000000000000'::uuid or v_target_id = p_device_id
        or exists (select 1 from public.project_members where project_route=p_project_route and device_id=v_target_id and status='active')
        or exists (select 1 from public.project_members where project_route=p_project_route and device_id=v_target_id and public_key<>v_target_key)
        or exists (select 1 from public.project_members where project_route=p_project_route and public_key=v_target_key and device_id<>v_target_id)
      then return jsonb_build_object('accepted',false); end if;

      select count(*) into v_active_count from public.project_members
        where project_route=p_project_route and status='active';
      if v_active_count >= 3 then return jsonb_build_object('accepted',false); end if;

      insert into public.project_members(
        project_route,device_id,public_key,role,status,granted_by_device_id,grant_seq,grant_signature,granted_at,
        revoked_by_device_id,revoke_seq,revocation_signature,revoked_at,last_action_seq
      ) values (
        p_project_route,v_target_id,v_target_key,'member','active',p_device_id,v_actor_seq,p_signature,p_issued_at,
        null,null,null,null,0
      )
      on conflict (project_route,device_id) do update set
        public_key=excluded.public_key, role='member', status='active',
        granted_by_device_id=excluded.granted_by_device_id, grant_seq=excluded.grant_seq,
        grant_signature=excluded.grant_signature, granted_at=excluded.granted_at,
        revoked_by_device_id=null, revoke_seq=null, revocation_signature=null, revoked_at=null;
      return jsonb_build_object('accepted',true);
    else
      if (select count(*) from jsonb_object_keys(p_body)) <> 2
        or not (p_body ?& array['actor_seq','target_device_id'])
        or (p_body->>'target_device_id') !~* '^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$'
      then raise exception using errcode = 'P0001', message = 'request rejected'; end if;
      v_target_id := (p_body->>'target_device_id')::uuid;
      if v_target_id = '00000000-0000-0000-0000-000000000000'::uuid then
        raise exception using errcode = 'P0001', message = 'request rejected';
      end if;
      if v_target_id <> p_device_id and v_member.role <> 'owner'
      then return jsonb_build_object('accepted',false); end if;
      update public.project_members set status='revoked',revoked_by_device_id=p_device_id,
        revoke_seq=v_actor_seq,revocation_signature=p_signature,revoked_at=p_issued_at
      where project_route=p_project_route and device_id=v_target_id and status='active';
      if not found then return jsonb_build_object('accepted',false); end if;
      delete from public.head_records where project_route=p_project_route and writer_device_id=v_target_id;
      return jsonb_build_object('accepted',true);
    end if;
  end if;

  if p_operation = 'endpoint-publish' then
    if (select count(*) from jsonb_object_keys(p_body)) <> 3
      or not (p_body ?& array['seq','expires_at','ciphertext'])
      or jsonb_typeof(p_body->'seq') <> 'string' or p_body->>'seq' !~ '^[1-9][0-9]{0,19}$'
      or jsonb_typeof(p_body->'expires_at') <> 'string'
      or jsonb_typeof(p_body->'ciphertext') <> 'string'
      or p_body->>'ciphertext' !~ '^[A-Za-z0-9_-]+$' or octet_length(p_body->>'ciphertext') > 65536
    then raise exception using errcode = 'P0001', message = 'request rejected'; end if;
    v_record_seq := (p_body->>'seq')::numeric;
    v_expiry := (p_body->>'expires_at')::timestamptz;
    v_ciphertext := p_body->>'ciphertext';
    if v_record_seq > 18446744073709551615
      or v_expiry <= clock_timestamp() or v_expiry > clock_timestamp() + interval '15 minutes'
      or v_record_seq <= v_member.last_endpoint_seq
    then return jsonb_build_object('accepted',false); end if;
    update public.project_members set last_endpoint_seq=v_record_seq
      where project_route=p_project_route and device_id=p_device_id;
    insert into public.endpoint_records(project_route,writer_device_id,seq,ciphertext,expires_at)
      values (p_project_route,p_device_id,v_record_seq,v_ciphertext,v_expiry);
    return jsonb_build_object('accepted',true);
  end if;

  if p_operation = 'endpoint-list' then
    if (select count(*) from jsonb_object_keys(p_body)) <> 1 or not (p_body ? 'after_cursor')
      or (p_body->'after_cursor' <> 'null'::jsonb and p_body->>'after_cursor' !~ '^(0|[1-9][0-9]{0,18})$')
    then raise exception using errcode = 'P0001', message = 'request rejected'; end if;
    v_cursor := coalesce(nullif(p_body->>'after_cursor','null')::bigint,0);
    if v_cursor < 0 then raise exception using errcode = 'P0001', message = 'request rejected'; end if;
    select coalesce(jsonb_agg(page.row_json order by page.cursor_id),'[]'::jsonb) into v_rows
    from (
      select cursor_id,jsonb_build_object(
        'cursor',cursor_id::text,'writer_device_id',writer_device_id,'seq',seq::text,
        'ciphertext',ciphertext,
        'expires_at',to_char(expires_at at time zone 'UTC','YYYY-MM-DD"T"HH24:MI:SS.MS"Z"'),
        'created_at',to_char(created_at at time zone 'UTC','YYYY-MM-DD"T"HH24:MI:SS.MS"Z"')
      ) as row_json
      from public.endpoint_records
      where project_route=p_project_route and expires_at>clock_timestamp() and cursor_id>v_cursor
      order by cursor_id limit 8
    ) as page;
    return v_rows;
  end if;

  if p_operation = 'head-publish' then
    if (select count(*) from jsonb_object_keys(p_body)) <> 2
      or not (p_body ?& array['seq','ciphertext'])
      or jsonb_typeof(p_body->'seq') <> 'string' or p_body->>'seq' !~ '^[1-9][0-9]{0,19}$'
      or jsonb_typeof(p_body->'ciphertext') <> 'string'
      or p_body->>'ciphertext' !~ '^[A-Za-z0-9_-]+$' or length(p_body->>'ciphertext') > 4096
    then raise exception using errcode = 'P0001', message = 'request rejected'; end if;
    v_record_seq := (p_body->>'seq')::numeric;
    v_ciphertext := p_body->>'ciphertext';
    if v_record_seq > 18446744073709551615 then return jsonb_build_object('accepted',false); end if;
    insert into public.head_records(project_route,writer_device_id,seq,ciphertext)
      values (p_project_route,p_device_id,v_record_seq,v_ciphertext)
      on conflict (project_route,writer_device_id) do update set seq=excluded.seq,ciphertext=excluded.ciphertext,updated_at=clock_timestamp()
      where public.head_records.seq < excluded.seq;
    if not found then return jsonb_build_object('accepted',false); end if;
    return jsonb_build_object('accepted',true);
  end if;

  if p_operation = 'head-list' then
    if (select count(*) from jsonb_object_keys(p_body)) <> 0
    then raise exception using errcode = 'P0001', message = 'request rejected'; end if;
    select coalesce(jsonb_agg(jsonb_build_object(
      'writer_device_id',writer_device_id,'seq',seq::text,'ciphertext',ciphertext,
      'updated_at',to_char(updated_at at time zone 'UTC','YYYY-MM-DD"T"HH24:MI:SS.MS"Z"')
    ) order by writer_device_id),'[]'::jsonb) into v_rows
    from (select writer_device_id,seq,ciphertext,updated_at from public.head_records
      where project_route=p_project_route order by updated_at desc limit 32) as rows;
    return v_rows;
  end if;
  raise exception using errcode = 'P0001', message = 'request rejected';
end;
$$;
revoke all on function public.trackmesh_dispatch(text,uuid,uuid,text,timestamptz,uuid,text,text,jsonb) from public, anon, authenticated;
grant execute on function public.trackmesh_dispatch(text,uuid,uuid,text,timestamptz,uuid,text,text,jsonb) to service_role;
commit;
