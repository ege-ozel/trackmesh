import { invoke } from '@tauri-apps/api/core';

export interface SnapshotInfo {
  project_id: string;
  snapshot_id: string; author_device_id: string; created_at: string;
  author_name: string; metadata_status: 'PENDING' | 'PUBLISHED' | 'REMOTE';
  total_size: number; changed_bytes: number; file_count: number;
}
export interface Peer {
  device: { device_id: string; name: string; peer_id: string };
  /** Newest version this collaborator signed. */
  announcement: SnapshotInfo | null;
  /** The collaborator's current version from the last head pull; another device may have signed it. */
  head: SnapshotInfo | null;
  /** A head pull has answered, also when the collaborator has no version (`fetched_head` null). */
  head_fetched: boolean;
  fetched_head: string | null;
  history: SnapshotInfo[];
  known_lineage: Lineage | null;
  /** The last version of this PC's the collaborator confirmed having. */
  delivered_snapshot?: string | null;
  offline_head: { snapshot_id: string; created_at: string; total_size: number; file_count: number } | null;
}
export interface Transfer {
  transfer_id: string; snapshot_id: string; source_device_id: string;
  state: 'QUEUED' | 'RECEIVING' | 'PAUSED' | 'APPLYING' | 'RECOVERY_REQUIRED' | 'COMPLETE';
  completed_bytes: number; total_bytes: number; error: string | null;
}
export interface ProjectCard {
  project: { project_id: string; name: string; root: string; latest_local_snapshot: string | null;
    last_verified_at: string | null; verified_peer_id: string | null; verified_snapshot_id: string | null;
    recovery_required: boolean; error: string | null };
  grant_problem: 'REJECTED' | 'UNREACHABLE' | null;
  revoke_problem: boolean;
  missing_functions: string[];
  root_available: boolean;
  root_empty: boolean;
  active_transfer: boolean;
  local_snapshot: SnapshotInfo | null; peers: Peer[]; transfer: Transfer | null;
}
export type ProjectBackend = 'NONE' | 'SELF_HOSTED_MAILBOX' | 'SUPABASE';
export interface ProjectBackendConfig {
  backend: ProjectBackend;
  supabase_url: string | null;
  supabase_anon_key: string | null;
}
export interface SupabaseDefaultConfig { url: string; publishable_key: string }
export interface Connection { online: boolean; route: 'DIRECT' | 'OFFLINE'; transport: string }
export interface FirewallObservation {
  state: string;
  profiles: { name: string; enabled: boolean | null; block_all_inbound: boolean | null; default_inbound: string | null }[];
  tcp_rules: { allow: string; block: string };
  udp_rules: { allow: string; block: string };
  third_party_products: string;
}
export type Lineage = 'EQUAL' | 'REMOTE_AHEAD' | 'LOCAL_AHEAD' | 'DIVERGED' | 'UNKNOWN';
export interface Job { job_id: string; project_id: string | null; label: string; running: boolean; outcome: Record<string, unknown> | null; error: string | null; completed_at: number | null }
export interface CheckView { device_id: string; fresh: boolean; current_known: boolean; age_seconds: number; result: {
  project_id: string; local_snapshot: string | null; remote_snapshot: string | null;
  lineage: Lineage | null;
  connection_state: 'CONNECTED' | 'OFFLINE';
  snapshot_status: 'REMOTE_AHEAD' | 'UP_TO_DATE' | 'DIVERGED' | 'LOCAL_AHEAD' | 'UNKNOWN';
  verified_equal: boolean; connection: Connection | null; last_verified_at: string | null;
} }
export interface NetworkObservation {
  source: string;
  interfaces: { interface_index: number; name: string; ipv4: string[]; gateways: string[] }[];
  default_gateways: { interface_index: number; address: string; source: string }[];
  wan_visible_ip: { state: 'UNKNOWN'; source: string; value: string | null };
  mappings: { method: 'UPNP_IGD' | 'PCP'; transport: 'TCP' | 'UDP'; state: 'NOT_ATTEMPTED' }[];
}
export interface DesktopStatus {
  presence: Record<string, { device_id: string; name: string; since: number; both_working: boolean; line: string; get_line: string }[]>;
  daw_plugin_settings: { project_id: string; name: string; auto_share: boolean }[];
  plugin_handoff: { revision: number; project_id: string | null; offset: number } | null;
  pairing: { project_id: string | null; remaining_seconds: number; requests: { request_id: string; device_id: string; name: string; peer_id: string }[];
    /** Projects a device this PC already works with offered to it. */
    offers: { project_id: string; project_name: string; device_id: string; name: string; existing_root: string | null }[] };
  listen_port: number; configured_external_addresses: string[];
  ready: boolean; closing: boolean; error: string | null;
  device: { device_id: string; name: string; peer_id: string; public_key?: string } | null;
  data_directory: string | null;
  projects_location: string;
  removed_projects: { project_id: string; name: string; root: string }[];
  network: { addresses: string[]; error: string | null; peers: Record<string, Connection>; received_snapshots: { peer_id: string; project_id: string; snapshot_id: string }[]; port_mapping: { consent_enabled: boolean; state: string; tcp: string; udp: string; detail: string | null; external_ip_state: string }; reachability: { state: string; peer: string | null; transport: string | null; at_unix_seconds: number | null } } | null;
  peer_states: Record<string, 'CONNECTED' | 'OFFLINE'>;
  port_status: 'UNVERIFIED' | null;
  pairing_approvals: { project_id: string; device_id: string; state: 'UNKNOWN' | 'PENDING' | 'NOT_REACHABLE' | 'APPROVED' }[];
  /** First-version head pulls that failed twice in a row. */
  head_pull_errors: { project_id: string; device_id: string; error: string }[];
  network_observation: NetworkObservation;
  firewall_observation: FirewallObservation;
  page: { projects: ProjectCard[]; offset: number; has_more: boolean } | null;
  jobs: Job[]; checks: CheckView[];
  project_backends: Record<string, ProjectBackendConfig>;
  /** Projects this PC joined from an invite; the rest it created. */
  joined_projects: string[];
  /** Per created project, devices this PC shares another project with that can be added without a link. */
  offerable_devices: Record<string, { device_id: string; name: string; peer_id: string }[]>;
  default_supabase: SupabaseDefaultConfig | null;
  supabase_states: Record<string, 'NOT_CONFIGURED' | 'UNKNOWN' | 'CLAIMED' | 'MEMBER' | 'ROUTE_TAKEN' | 'WAITING_FOR_GRANT' | 'UNREACHABLE' | 'REMOVED'>;
  invite_pending: boolean;
  invite_needs_service_fields: boolean;
  upnp_mapping_asked: boolean;
  manual_public_endpoint: { public_ipv4: string; external_port: number } | null;
}
export type Action =
  | { kind: 'set_auto_share'; project_id: string; enabled: boolean }
  | { kind: 'delete_backups'; selected: { transfer_id: string; path: string; bytes: number }[] }
  | { kind: 'link'; project_id: string; name: string; folder: string }
  | { kind: 'change_project_root'; project_id: string; folder: string }
  | { kind: 'set_project_backend'; project_id: string; backend: ProjectBackend; supabase_url: string | null; supabase_anon_key: string | null }
  | { kind: 'set_default_supabase'; url: string | null; publishable_key: string | null }
  | { kind: 'set_device_name'; name: string }
  | { kind: 'set_projects_location'; location: string }
  | { kind: 'set_manual_public_endpoint'; public_ipv4: string | null; external_port: number | null }
  | { kind: 'join'; folder: string }
  | { kind: 'complete_invite_link'; supabase_url?: string; supabase_anon_key?: string }
  | { kind: 'export_card' }
  | { kind: 'open_pairing' | 'close_pairing'; project_id: string }
  | { kind: 'approve_pairing' | 'reject_pairing'; project_id: string; request_id: string }
  | { kind: 'request_pairing'; project_id: string; device_id: string }
  | { kind: 'offer_project'; project_id: string; device_id: string }
  | { kind: 'accept_offer' | 'decline_offer'; project_id: string }
  | { kind: 'pair' | 'send'; project_id: string }
  | { kind: 'export_invite'; project_id: string; public_ip?: string | null; external_port?: number | null }
  | { kind: 'copy_invite_link'; project_id: string }
  | { kind: 'revoke' | 'check'; project_id: string; device_id: string }
  | { kind: 'remove_project'; project_id: string; delete_data: boolean }
  | { kind: 'restore_project'; project_id: string }
  | { kind: 'receive'; project_id: string; device_id: string; snapshot_id: string | null; daw_closed: boolean }
  | { kind: 'use_peer_version'; project_id: string; device_id: string; daw_closed: boolean }
  | { kind: 'recover'; project_id: string; daw_closed: boolean };

export const getStatus = (offset: number) => invoke<DesktopStatus>('get_status', { offset });
export type StorageSummary = { cache_bytes: number | null; backups: { project_id: string; project_name: string; transfer_id: string; path: string; bytes: number; modified_at: number | null; protected: boolean }[]; problems: string[] };
export const getStorage = () => invoke<StorageSummary>('get_storage');
export type SnapshotChanges = { added: string[]; removed: string[]; changed: string[]; more: boolean };
export const snapshotChanges = (projectId: string, snapshotId: string) => invoke<SnapshotChanges>('snapshot_changes', { projectId, snapshotId });
export const chooseFolder = () => invoke<string | null>('choose_folder');
export const openProjectFolder = (projectId: string) => invoke<void>('open_project_folder', { projectId });
export const openPreviousVersion = (projectId: string, transferId: string) => invoke<void>('open_previous_version', { projectId, transferId });
export const openProjectsLocation = () => invoke<void>('open_projects_location');
export const chooseProjectsLocation = () => invoke<string | null>('choose_projects_location');
export const chooseInviteParentFolder = () => invoke<string | null>('choose_invite_parent_folder');
export const openInviteParentFolder = () => invoke<void>('open_invite_parent_folder');
export const pasteInviteLink = () => invoke<void>('paste_invite_link');
export const cancelInviteLink = () => invoke<void>('cancel_invite_link');
/** Stops the project's running Check, Share or Get at its next safe point. Installing files always finishes. */
export const stopOperation = (projectId: string) => invoke<void>('stop_operation', { projectId });
export const submitAction = (action: Action) => invoke<string>('submit_action', { action });
export const setUpnpMapping = (enabled: boolean) => invoke<void>('set_upnp_mapping', { enabled });
// One question at a time: a second click while a question is open counts as Cancel.
let asking = false;
export const ask = async (title: string, message: string, ok: string, cancel: string) => {
  if (asking) return false;
  asking = true;
  try { return await invoke<boolean>('ask', { title, message, ok, cancel }); }
  finally { asking = false; }
};
export const setLanguage = (lang: string) => invoke<void>('set_language', { lang });
export const closeApp = () => invoke<void>('close_app');
