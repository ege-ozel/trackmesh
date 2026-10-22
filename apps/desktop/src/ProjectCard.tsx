import { useState } from 'react';
import { ask, chooseFolder, openPreviousVersion, openProjectFolder, stopOperation } from './api';
import type { Action, DesktopStatus, ProjectCard } from './api';
import { ErrorDetails, InProgress, InviteCopiedNotice, JobNotice, StopButton, Waiting, friendlyError } from './Jobs';
import { bytes, displayPath, short, time } from './format';
import { named, t } from './i18n';

// The collaborator's name inside a sentence, in the Connected badge's green so it is easy to pick out.
// A Turkish suffix after an apostrophe (B'nin, B'den) stays with the name.
export const mark = (text: string, name?: string) => !name ? text
  : text.split(new RegExp(`(${name.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')}(?:'\\p{L}+)?)`, 'u'))
    .map((part, i) => i % 2 ? <span key={i} className="who">{part}</span> : part);

export type AdvancedSection = 'connection' | 'service' | 'manual' | 'history' | 'access';

type Problem = { tone: 'warn' | 'danger'; title: string; body: string; fix: string; section: AdvancedSection };

// Per project, the paused version the last Resume getting continued, so its Cancel keeps that
// version pinned. Kept outside the card, which unmounts when another project is shown.
const resumedFrom = new Map<string, string | null>();
const RECEIVE_LABELS = ["Get collaborator's version", "Use collaborator's version", 'Recover installation'];

type TabTone = 'attention' | 'new' | 'warn' | 'ok' | 'idle';

// What a project tab's dot says, most urgent first, so hidden projects still show what needs you.
export function projectState(card: ProjectCard, status: DesktopStatus): { tone: TabTone; label: string } {
  const id = card.project.project_id;
  const online = (peer: ProjectCard['peers'][number]) => status.network?.peers[peer.device.peer_id]?.online ?? false;
  const job = status.jobs.find(j => j.project_id === id);
  if (status.pairing.project_id === id && status.pairing.requests.length) return { tone: 'attention', label: t('Someone is waiting for your approval', 'Onayını bekleyen biri var') };
  // While a Get puts files in place the folder and the recovery record are briefly in between; judge them after it ends.
  if (!job?.running && !card.root_available) return { tone: 'attention', label: t('Project folder not found', 'Proje klasörü bulunamadı') };
  if (!job?.running && card.project.recovery_required) return { tone: 'attention', label: t('The last Get did not finish', 'Son alma işlemi bitmedi') };
  if (card.peers.some(peer => peer.known_lineage === 'DIVERGED') || (!job?.running && job?.outcome?.receive_blocked)) return { tone: 'attention', label: t('Choose which version to keep', 'Hangi sürümün kalacağını seç') };
  const newVersion = card.transfer?.state === 'PAUSED' || card.peers.some(peer => online(peer) ? peer.known_lineage === 'REMOTE_AHEAD'
    : !!peer.offline_head && (!peer.announcement || Date.parse(peer.offline_head.created_at) > Date.parse(peer.announcement.created_at)));
  if (newVersion) return { tone: 'new', label: t('New version to get', 'Alınacak yeni sürüm var') };
  if (status.supabase_states[id] === 'REMOVED') return { tone: 'warn', label: t('You were removed from this project', 'Bu projeden çıkarıldın') };
  if (!card.peers.length) return { tone: 'idle', label: t('No collaborator yet', 'Henüz iş arkadaşı yok') };
  if (status.pairing_approvals.some(item => item.project_id === id && item.state === 'PENDING')) return { tone: 'idle', label: t('Waiting for approval', 'Onay bekleniyor') };
  // A refused request reads as unavailable here, like a closed approval window or a network error,
  // so it only says approval could not be confirmed, and only when no collaborator here has it.
  const approvals = card.peers.map(peer => status.pairing_approvals.find(item => item.project_id === id && item.device_id === peer.device.device_id)?.state);
  if (approvals.length && approvals.every(state => state === 'NOT_REACHABLE')) return { tone: 'warn', label: t('Approval not confirmed', 'Onay doğrulanamadı') };
  return card.peers.some(online) ? { tone: 'ok', label: t('Connected', 'Bağlı') } : { tone: 'warn', label: t("Can't reach your collaborator", 'İş arkadaşına ulaşılamıyor') };
}

export function ProjectTabs({ projects, status, selectedId, pick }: {
  projects: ProjectCard[]; status: DesktopStatus; selectedId: string; pick: (projectId: string) => void;
}) {
  return <div className="project-chips" role="tablist" aria-label={t('Projects', 'Projeler')}>{projects.map(item => {
    const id = item.project.project_id;
    const state = projectState(item, status);
    return <button key={id} role="tab" aria-selected={id === selectedId} className={id === selectedId ? 'chip on' : 'chip'} title={state.label} aria-label={`${item.project.name} · ${state.label}`} onClick={() => pick(id)}>
      <span className={`tab-dot ${state.tone}`} aria-hidden="true" />{item.project.name}</button>;
  })}</div>;
}

export function Card({ card, status, jobStartedAt, focusProjectId, act, openAdvanced }: {
  card: ProjectCard; status: DesktopStatus; jobStartedAt: Record<string, number>; focusProjectId: string | null;
  act: (action: Action) => Promise<boolean>; openAdvanced: (projectId: string, section: AdvancedSection) => void;
}) {
  const [selected, setSelected] = useState('');
  const [folderError, setFolderError] = useState('');
  const [keepMinePending, setKeepMinePending] = useState(false);
  // The remove panel: null closed, otherwise whether to delete this PC's versions and key too.
  const [removing, setRemoving] = useState<boolean | null>(null);
  const project = card.project;
  const projectId = project.project_id;
  const peer = card.peers.find(p => p.device.device_id === selected) ?? card.peers[0];
  const peerId = peer?.device.device_id;
  const collab = peer?.device.name;
  const peerName = collab ?? 'your collaborator';
  const pairingApproval = peer ? status.pairing_approvals.find(item => item.project_id === projectId && item.device_id === peer.device.device_id)?.state ?? 'UNKNOWN' : 'UNKNOWN';
  const online = (peer ? status.network?.peers[peer.device.peer_id]?.online : false) ?? false;
  // The backend keeps one job per project: the running one or the latest result.
  const job = status.jobs.find(j => j.project_id === projectId);
  const busy = !status.ready || status.closing || status.jobs.some(j => j.running && (!j.project_id || j.project_id === projectId));
  const check = status.checks.find(c => c.result.project_id === projectId && c.device_id === peerId);
  const divergence = (check?.fresh && check.result.snapshot_status === 'DIVERGED') || peer?.known_lineage === 'DIVERGED';
  const blockedCode = job?.outcome?.receive_blocked;
  const resolveDivergence = divergence || blockedCode === 'DIVERGED';
  const resolveLocalChanges = blockedCode === 'LOCAL_CHANGES';
  const usedTransferId = job && job.label === "Use collaborator's version" && !job.running && !job.error && typeof job.outcome?.transfer_id === 'string' ? String(job.outcome.transfer_id) : null;
  const peerHasVersion = card.peers.some(item => !!item.announcement || !!item.offline_head || !!item.known_lineage);
  const firstShareBlocked = !card.local_snapshot && (card.root_empty || peerHasVersion);
  // The collaborator's current version; its newest own one is only known before a head pull.
  const peerVersion = peer?.head ?? peer?.announcement;
  const remoteAhead = online && peer?.known_lineage === 'REMOTE_AHEAD';
  const offlineHead = peer?.offline_head;
  const showOfflineHead = !!offlineHead && !!peer && !online && !remoteAhead
    && (!peer.announcement || Date.parse(offlineHead.created_at) > Date.parse(peer.announcement.created_at));
  const transfer = card.transfer;
  // A new Get that stopped at its own question (Cancel in the change list, or refused for unshared
  // changes) fetched nothing: the card shows Get, not a paused download. A Resume getting that was
  // cancelled stays a paused download of the same pinned version.
  const setAside = transfer?.state === 'PAUSED' && !!job && !job.running && RECEIVE_LABELS.includes(job.label)
    && (!!job.outcome?.receive_blocked || /Download cancelled/i.test(job.error ?? ''))
    && resumedFrom.get(projectId) !== transfer.snapshot_id;
  const resumable = transfer?.state === 'PAUSED' && peerId === transfer.source_device_id && !setAside;
  const receiving = transfer?.state === 'RECEIVING' || transfer?.state === 'APPLYING';
  const upToDate = online && !resumable && (peer?.known_lineage === 'EQUAL' || peer?.known_lineage === 'LOCAL_AHEAD');
  const percent = transfer?.total_bytes ? Math.min(100, Math.round(transfer.completed_bytes * 100 / transfer.total_bytes)) : 0;
  // A version made only of empty files has 0 bytes to download: show it as done once download is
  // over, and as indeterminate while TrackMesh still works out the size.
  const noFileData = !!transfer && transfer.total_bytes === 0;
  const transferDone = transfer?.state === 'APPLYING' || transfer?.state === 'COMPLETE' || transfer?.state === 'RECOVERY_REQUIRED';
  const authoredHere = card.local_snapshot?.author_device_id === status.device?.device_id;
  const receipt = !!authoredHere && !!peer && !!card.local_snapshot && (
    (project.verified_peer_id === peer.device.peer_id && project.verified_snapshot_id === card.local_snapshot.snapshot_id)
    || !!status.network?.received_snapshots.some(item => item.project_id === projectId && item.peer_id === peer.device.peer_id && item.snapshot_id === card.local_snapshot?.snapshot_id)
  );
  // A version the plugin shared has no Desktop job; this PC being ahead with its own version means it was shared.
  const sharedLatest = !!card.local_snapshot && (job?.outcome?.snapshot_id === card.local_snapshot.snapshot_id
    || (authoredHere && peer?.known_lineage === 'LOCAL_AHEAD'));
  const headPullError = peer ? status.head_pull_errors.find(item => item.project_id === projectId && item.device_id === peer.device.device_id)?.error : undefined;
  const waitingFirstVersion = !!peer && card.root_empty && !card.local_snapshot && !project.latest_local_snapshot
    && !project.recovery_required && !peer.announcement
    && (!transfer || transfer.state === 'COMPLETE')
    && !job?.running
    && pairingApproval !== 'PENDING' && pairingApproval !== 'NOT_REACHABLE';
  // A running Get briefly moves files and keeps a recovery record open; only a finished job's state counts.
  const rootMissing = !job?.running && !card.root_available;
  const recoveryNeeded = !job?.running && project.recovery_required;
  const handoff = status.plugin_handoff?.project_id === projectId;
  // Only the PC that created a project can take a collaborator off the invite service; others leave.
  const creator = !status.joined_projects.includes(projectId);
  const backend = status.project_backends[projectId]?.backend;
  const inviteState = status.supabase_states[projectId] ?? 'NOT_CONFIGURED';
  const inviteReady = backend === 'SUPABASE' && ['CLAIMED', 'MEMBER'].includes(inviteState);
  // The invite service refuses this member since the creator removed it; P2P then only looks unreachable.
  const removedHere = !creator && inviteState === 'REMOVED';
  const leaving = !!peer && !creator && !removedHere;
  // Devices from this PC's other projects can be added without a link, up to 3 devices per project.
  const offerable = creator && card.peers.length < 2 ? status.offerable_devices[projectId] ?? [] : [];
  const offer = async (device: { device_id: string; name: string }, id: string) => await ask(t(`Add ${device.name} to ${project.name}?`, `${named(device.name, 'acc')} ${project.name} projesine ekle?`),
    t(`${device.name}${id} already works with you on another project. They get ${project.name} once they accept on their PC; no invite link is needed.`,
      `${device.name}${id} seninle başka bir projede zaten çalışıyor. Kendi bilgisayarında kabul edince ${project.name} projesini alır; davet bağlantısı gerekmez.`),
    t('Add', 'Ekle'), t('Cancel', 'Vazgeç')).catch(() => false) && act({ kind: 'offer_project', project_id: projectId, device_id: device.device_id });
  // A device with the same name as a collaborator already here is another PC (often a reinstall).
  const sameName = (device: { name: string }) => card.peers.some(item => item.device.name === device.name);
  // Offered devices that share a name (an old and a new install) carry their short device ID,
  // the same one the approval box shows, so the buttons can be told apart.
  const twin = (device: { name: string }) => offerable.filter(item => item.name === device.name).length > 1;
  const offerButtons = offerable.map(device => {
    const id = twin(device) ? t(` (device ${short(device.device_id)})`, ` (cihaz ${short(device.device_id)})`) : '';
    return <button key={device.device_id} className="text-button" disabled={busy} onClick={() => void offer(device, id)}>{sameName(device)
      ? t(`Add another PC named ${device.name}${id}`, `${device.name} adlı başka bir bilgisayarı ekle${id}`)
      : t(`Add ${device.name}${id}`, `${named(device.name, 'acc')} ekle${id}`)}</button>;
  });
  const getLine = peer && status.presence[projectId]?.find(warning => warning.device_id === peer.device.device_id)?.get_line;
  // The collaborator answered that it has no version yet: nothing to get.
  const peerEmpty = !!peer && peer.head_fetched && !peer.fetched_head;
  const disabledReceive = busy || removedHere || !peer || !online || upToDate || peerEmpty || project.recovery_required || divergence || rootMissing;
  const candidates = status.pairing.project_id === projectId ? status.pairing.requests : [];
  const pairingOpen = status.pairing.project_id === projectId;
  const inviteJob = job?.label === 'Copy invite link' ? job : undefined;
  const copyInviteSuccess = !!inviteJob && !inviteJob.running && !inviteJob.error && inviteJob.outcome?.invite_copied === true;
  const chooseProjectRoot = async () => {
    try {
      const folder = await chooseFolder();
      if (folder) await act({ kind: 'change_project_root', project_id: projectId, folder });
    } catch (err) { setFolderError(String(err)); }
  };
  const openProject = async () => {
    try { await openProjectFolder(projectId); }
    catch (err) { setFolderError(String(err)); }
  };
  const share = () => act({ kind: 'send', project_id: projectId });
  const stop = () => void stopOperation(projectId).catch(err => setFolderError(String(err)));
  // One question right before Recover changes files, instead of a checkbox to tick first.
  const dawClosed = (what: string) => ask(t('Is this project closed in your DAW?', "Proje DAW'da kapalı mı?"), what, t("Yes, it's closed", 'Evet, kapalı'), t('Cancel', 'Vazgeç'))
    .catch(err => { setFolderError(String(err)); return false; });
  // Get and Use version ask in their own window (changed files, then "close the project in your
  // DAW"), shown by the desktop before anything is downloaded, so Get asks nothing here.
  const receive = async () => {
    if (!peer) return false;
    const pinned = resumable ? transfer.snapshot_id : null;
    resumedFrom.set(projectId, pinned);
    return act({ kind: 'receive', project_id: projectId, device_id: peer.device.device_id, snapshot_id: pinned, daw_closed: true });
  };
  // Use version first saves the folder as a new version, which the collaborator can get, and only
  // then shows that window; so it asks here, before anything is saved.
  const usePeerVersion = async () => peer
    && await ask(t(`Use ${peerName}'s version?`, `${named(collab, 'gen')} sürümü kullanılsın mı?`),
      t(`If this folder has changes you haven't shared, TrackMesh first saves them as a new version of yours so nothing is lost. ${peerName} can get that version too.\n\nNext you see which files change.`,
        `Bu klasörde paylaşmadığın değişiklikler varsa TrackMesh önce onları senin yeni sürümün olarak kaydeder, böylece hiçbir şey kaybolmaz. ${peerName} da o sürümü alabilir.\n\nSonra hangi dosyaların değiştiğini görürsün.`),
      t('Save and continue', 'Kaydet ve devam et'), t('Cancel', 'Vazgeç')).catch(err => { setFolderError(String(err)); return false; })
    && act({ kind: 'use_peer_version', project_id: projectId, device_id: peer.device.device_id, daw_closed: true });
  const recover = async () => await dawClosed(t('TrackMesh will finish putting the files in place.', 'TrackMesh dosyaları yerleştirmeyi bitirecek.'))
    && act({ kind: 'recover', project_id: projectId, daw_closed: true });

  // One problem at a time, most blocking first; each routes to the Advanced section that can fix it.
  // A missing folder or an unfinished Get comes first; their own notice is the only one shown then.
  let problem: Problem | null = null;
  if (rootMissing || recoveryNeeded || removedHere) problem = null;
  else if (peer && pairingApproval !== 'PENDING' && pairingApproval !== 'NOT_REACHABLE' && !online && !(job?.running && job.label === 'Check connection'))
    problem = { tone: 'warn', title: t(`TrackMesh can't reach ${peerName}.`, `TrackMesh ${named(collab, 'dat')} ulaşamıyor.`),
      body: t(`Ask ${peerName} to open TrackMesh. If it still fails, check the connection.`, `${named(collab, 'dat')} TrackMesh'i açmasını söyle. Yine olmazsa bağlantıyı kontrol et.`),
      fix: t('Fix connection', 'Bağlantıyı düzelt'), section: 'connection' };
  else if (card.missing_functions.length)
    problem = { tone: 'danger', title: t('The invite service needs an update.', 'Davet servisinin güncellenmesi gerekiyor.'), body: t('Some invite service functions are missing.', 'Davet servisinde bazı fonksiyonlar eksik.'),
      fix: t('See invite service', 'Davet servisine bak'), section: 'service' };
  else if (card.grant_problem === 'UNREACHABLE' || inviteState === 'UNREACHABLE')
    problem = { tone: 'warn', title: t("The invite service can't be reached.", 'Davet servisine ulaşılamıyor.'),
      body: t('Direct sharing still works. Invite links and version notices need the service.', 'Doğrudan paylaşım çalışmaya devam eder. Davet bağlantıları ve sürüm bildirimleri için servis gerekir.'),
      fix: t('Check invite service', 'Davet servisini kontrol et'), section: 'service' };
  else if (inviteState === 'ROUTE_TAKEN')
    problem = { tone: 'danger', title: t("The invite service won't let this PC create invites.", 'Davet servisi bu bilgisayarın davet oluşturmasına izin vermiyor.'),
      body: t("Its owner list may not include this PC. Whoever runs the service adds this PC's public key (Advanced → This device). Direct sharing with paired collaborators still works.", "Sahip listesinde bu bilgisayar olmayabilir. Servisi işleten kişi bu bilgisayarın açık anahtarını (Gelişmiş → Bu cihaz) ekler. Eşleşmiş iş arkadaşlarıyla doğrudan paylaşım çalışmaya devam eder."),
      fix: t('See invite service', 'Davet servisine bak'), section: 'service' };
  else if (!peer && !inviteReady && inviteState !== 'UNKNOWN' && inviteState !== 'WAITING_FOR_GRANT')
    problem = { tone: 'warn', title: t('Invite links need the invite service.', 'Davet bağlantıları için davet servisi gerekir.'),
      body: t('Add your Supabase address once, then Invite collaborator works.', 'Supabase adresini bir kez ekle, sonra davet etme çalışır.'),
      fix: t('Set up invite service', 'Davet servisini kur'), section: 'service' };

  // One green button per card, by priority: Locate/Recover, Approve, the problem's fix, Get, Share, Invite.
  const getFirst = remoteAhead || resumable;
  const primary = removedHere ? 'remove' : rootMissing || recoveryNeeded ? 'repair' : candidates.length ? 'approve' : problem ? 'fix'
    : getFirst ? 'get' : peer && !firstShareBlocked ? 'share' : 'invite';

  // While the connection banner shows, the version waiting offline is one more line in it, not a second box.
  const offlineLine = showOfflineHead && offlineHead ? `${t(`New version from ${peerName}`, `${named(collab, 'abl')} yeni sürüm`)} · ${time(offlineHead.created_at)} · ${t(`available when ${peerName} is online`, `${peerName} bağlanınca alınabilir`)}` : '';
  const offlineInProblem = !!offlineLine && problem?.section === 'connection';

  // The problem banner names the collaborator; the badge then only says the state.
  const badge = removedHere ? { text: t('Removed', 'Çıkarıldın'), tone: 'warn' }
    : pairingApproval === 'PENDING' ? { text: t('Waiting for approval', 'Onay bekleniyor'), tone: '' }
    : pairingApproval === 'NOT_REACHABLE' ? { text: t('Approval not confirmed', 'Onay doğrulanamadı'), tone: 'warn' }
    : online ? { text: t(`Connected · ${peerName}`, `Bağlı · ${peerName}`), tone: 'online' }
    : peer ? { text: problem?.section === 'connection' ? t('Not connected', 'Bağlı değil') : t(`Not connected · ${peerName}`, `Bağlı değil · ${peerName}`), tone: 'warn' }
    : { text: t('No collaborator yet', 'Henüz iş arkadaşı yok'), tone: '' };

  const transferError = transfer?.error ?? (job?.error && RECEIVE_LABELS.includes(job.label) ? job.error : null);
  const transferBox = transfer && <div className="transfer">
    <div><span>{receiving
      ? (transfer.state === 'APPLYING' ? t("Putting the files in place… this can't be stopped", 'Dosyalar yerleştiriliyor… bu durdurulamaz') : t(`Getting ${peerName}'s version…`, `${named(collab, 'gen')} sürümü alınıyor…`))
      : transfer.state === 'PAUSED' ? t('Paused · press Resume getting to continue', "Duraklatıldı · devam etmek için Almaya devam et'e bas")
      : transfer.state === 'RECOVERY_REQUIRED' ? t('Not finished · press Recover above', "Bitmedi · yukarıdaki Kurtar'a bas")
      : t('Waiting to start…', 'Başlaması bekleniyor…')}</span>
      {/* Chunks this PC already holds count as done from the start, so a small change on a large project
          starts near 100 %. Saying what is left shows that only the changed part travels. */}
      <span>{noFileData ? (transferDone ? t('Only empty files · nothing to download', 'Yalnızca boş dosyalar · indirilecek bir şey yok') : '')
        : transferDone ? t('Downloaded · 100 %', 'İndirildi · %100')
        : t(`${bytes(Math.max(0, transfer.total_bytes - transfer.completed_bytes))} left · ${percent} %`, `${bytes(Math.max(0, transfer.total_bytes - transfer.completed_bytes))} kaldı · %${percent}`)}</span></div>
    {noFileData && !transferDone ? <progress /> : <progress max={noFileData ? 1 : transfer.total_bytes} value={noFileData ? 1 : transfer.completed_bytes} />}
    {transfer.state === 'RECEIVING' && job?.running && <div className="actions"><StopButton key={job.job_id} onStop={stop} /></div>}
    {transferError && <><p className="error">{friendlyError(transferError, collab) ?? transferError}</p><ErrorDetails raw={transferError} collaboratorName={collab} /></>}</div>;

  // A single status line per card, by priority: work running now, an unfinished Get, the latest
  // result while it is fresh, then what the card is waiting for. Errors stay until the next action.
  const nowSeconds = Math.floor(Date.now() / 1000);
  const resultFresh = !!job && !job.running && !job.outcome?.receive_blocked && (!!job.error || job.outcome?.pairing_state === 'retry' || !!usedTransferId
    || nowSeconds - (job.completed_at ?? nowSeconds) < (copyInviteSuccess ? 75 : 20));
  const statusLine = transfer && receiving ? transferBox
    : inviteJob?.running ? <InProgress>{t('Creating invite link…', 'Davet bağlantısı oluşturuluyor…')}</InProgress>
    : job?.running ? <JobNotice job={job} startedAt={jobStartedAt[job.job_id]} collaboratorName={collab} online={online} onStop={stop} />
    : transfer && transfer.state !== 'COMPLETE' && !setAside ? transferBox
    : job && resultFresh ? (usedTransferId
        ? <div className="delivery-receipt" role="status">{mark(t(`Now using ${peerName}'s version ✓`, `Artık ${named(collab, 'gen')} sürümü kullanılıyor ✓`), collab)} <button className="text-button" onClick={() => void openPreviousVersion(projectId, usedTransferId).catch(error => setFolderError(String(error)))}>{t('Open my previous files', 'Önceki dosyalarımı aç')}</button></div>
        : copyInviteSuccess ? <><InviteCopiedNotice job={job} />
          {typeof job.outcome?.cleared_pending === 'string' && <div className="job" role="status">{t(`Pending approvals for ${job.outcome.cleared_pending} were cleared; only one project can accept new devices at a time.`, `${job.outcome.cleared_pending} için bekleyen onaylar silindi; aynı anda yalnızca bir proje yeni cihaz kabul edebilir.`)}</div>}</>
        : <JobNotice job={job} startedAt={jobStartedAt[job.job_id]} collaboratorName={collab} online={online} />)
    : peer && pairingApproval === 'PENDING' ? <Waiting>{t(`Waiting for ${peerName} to approve this device…`, `${named(collab, 'gen')} bu cihazı onaylaması bekleniyor…`)}</Waiting>
    : peer && waitingFirstVersion && headPullError ? <div className="notice danger" role="alert"><strong>{t(`Couldn't get ${peerName}'s first version.`, `${named(collab, 'gen')} ilk sürümü alınamadı.`)}</strong><p>{friendlyError(headPullError, collab) ?? headPullError}</p><ErrorDetails raw={headPullError} collaboratorName={collab} /></div>
    : peer && waitingFirstVersion ? <Waiting>{t(`Waiting for ${peerName} to share the first version…`, `${named(collab, 'gen')} ilk sürümü paylaşması bekleniyor…`)}</Waiting>
    : peer && receipt && online && !resolveDivergence && !resolveLocalChanges ? <div className="delivery-receipt" role="status">{mark(t(`Received by ${peerName} ✓`, `${peerName} aldı ✓`), collab)}</div>
    : peer && authoredHere && sharedLatest && online ? <Waiting>{t(`Shared. ${peerName} hasn't got this version yet.`, `Paylaşıldı. ${peerName} bu sürümü henüz almadı.`)}</Waiting>
    : null;

  return <article id={`project-${projectId}`} className={`project-card${focusProjectId === projectId ? ' focus-highlight' : ''}`}>
    <header className="card-title">
      <div className="card-name"><h2>{project.name}</h2><button className="path-link" title={t('Open in Explorer', "Gezgin'de aç")} onClick={() => void openProject()}>{displayPath(project.root)}</button></div>
      <span className={`badge ${badge.tone}`}>{badge.text}</span>
    </header>

    {rootMissing && <section className="notice danger"><strong>{t('Project folder not found', 'Proje klasörü bulunamadı')}</strong><p>{t('It may have been moved, renamed or on a drive that is not connected.', 'Taşınmış, adı değişmiş ya da bağlı olmayan bir sürücüde olabilir.')}</p><div className="actions"><button className="primary" disabled={busy} onClick={() => void chooseProjectRoot()}>{t('Locate folder', 'Klasörü bul')}</button></div></section>}
    {recoveryNeeded && <section className="notice danger"><strong>{t('The last Get did not finish.', 'Son alma işlemi bitmedi.')}</strong><p>{t('Close this project in your DAW, then recover it before getting another version.', "Başka bir sürüm almadan önce projeyi DAW'da kapat ve kurtar.")}</p>
      <div className="actions"><button className={rootMissing ? '' : 'primary'} disabled={busy} onClick={() => void recover()}>{t('Recover', 'Kurtar')}</button></div></section>}
    {removedHere && removing === null && <section className="notice warn" role="alert"><strong>{mark(t(`${peerName} removed this PC from ${project.name}.`, `${peerName} bu bilgisayarı ${project.name} projesinden çıkardı.`), collab)}</strong>
      <p>{t('The files you already got stay in the folder. To work together again, ask for a new invite.', 'Aldığın dosyalar klasörde kalır. Yeniden birlikte çalışmak için yeni bir davet iste.')}</p>
      <div className="actions"><button className="primary" disabled={busy} onClick={() => setRemoving(false)}>{t('Remove from TrackMesh', "TrackMesh'ten kaldır")}</button></div></section>}
    {problem && <section className={`problem ${problem.tone}`} role="alert"><div><strong>{problem.title}</strong><p>{problem.body}</p>{offlineInProblem && <p>{mark(offlineLine, collab)}</p>}</div>
      <button className={primary === 'fix' ? 'primary' : ''} onClick={() => openAdvanced(projectId, problem.section)}>{problem.fix} →</button></section>}
    {!peer && inviteState === 'WAITING_FOR_GRANT' && <Waiting>{t("Waiting for your collaborator's TrackMesh to confirm this device with the invite service. Keep both apps open.", "İş arkadaşının TrackMesh'inin bu cihazı davet servisine onaylaması bekleniyor. İki uygulamayı da açık tut.")}</Waiting>}
    {!peer && backend === 'SUPABASE' && inviteState === 'UNKNOWN' && <Waiting>{t('Connecting to the invite service…', 'Davet servisine bağlanılıyor…')}</Waiting>}
    {status.presence[projectId]?.map(warning => <p key={warning.device_id} className={`notice ${warning.both_working ? 'danger' : 'warn'}`} role="status">
      {warning.line}
      {!warning.both_working && ` (${t('since', 'başlangıç')} ${new Date(warning.since * 1000).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit', hour12: false })})`}</p>)}
    {folderError && <p className="error">{folderError}</p>}

    {candidates.length > 0 && <section className="approval-box"><h3>{t('Someone opened your invite link', 'Biri davet bağlantını açtı')}</h3>{candidates.map(candidate => <div key={candidate.request_id}>
      <p><strong>{candidate.name}</strong> · {t('device', 'cihaz')} {short(candidate.device_id)}</p>
      <div className="actions"><button className={primary === 'approve' ? 'primary' : ''} disabled={busy} onClick={async () => await ask(t('Approve this device?', 'Bu cihaz onaylansın mı?'),
          t(`${candidate.name} (device ${short(candidate.device_id)}) will get ${project.name}.\n\nApprove only if this is the person you invited.`, `${candidate.name} (cihaz ${short(candidate.device_id)}) ${project.name} projesini alacak.\n\nYalnızca davet ettiğin kişiyse onayla.`),
          t('Approve', 'Onayla'), t('Cancel', 'Vazgeç')).catch(() => false) && act({ kind: 'approve_pairing', project_id: projectId, request_id: candidate.request_id })}>{t('Approve', 'Onayla')}</button>
        <button disabled={busy} onClick={() => act({ kind: 'reject_pairing', project_id: projectId, request_id: candidate.request_id })}>{t('Reject', 'Reddet')}</button></div></div>)}</section>}

    {!peer && !candidates.length && <section className="invite-flow"><h3>{t('Invite your collaborator', 'İş arkadaşını davet et')}</h3>
      <p className="muted">{t('Copies a private link. Send it to one person; it works once, for 15 minutes.', 'Özel bir bağlantı kopyalar. Tek bir kişiye gönder; bir kez ve 15 dakika boyunca çalışır.')}</p>
      <button className={primary === 'invite' ? 'primary' : ''} disabled={busy || !inviteReady} onClick={() => act({ kind: 'copy_invite_link', project_id: projectId })}>{t('Invite collaborator', 'Davet et')}</button>
      {offerButtons.length > 0 && <p className="muted">{t('Or add someone you already share a project with:', 'Ya da başka bir projede birlikte çalıştığın birini ekle:')} {offerButtons}</p>}</section>}
    {pairingOpen && !candidates.length && <Waiting action={<button disabled={busy} onClick={() => act({ kind: 'close_pairing', project_id: projectId })}>{t('Stop waiting', 'Beklemeyi bırak')}</button>}>
      {t('Waiting for your collaborator to open the link', 'İş arkadaşının bağlantıyı açması bekleniyor')}{status.pairing.remaining_seconds > 0 ? ` · ${Math.ceil(status.pairing.remaining_seconds / 60)} ${t('min left', 'dk kaldı')}` : ''}. {t('Keep TrackMesh open.', "TrackMesh'i açık tut.")}</Waiting>}
    {/* Only once the 15-minute window has run out; before that the waiting line covers it. */}
    {!pairingOpen && !candidates.length && !!inviteJob && !inviteJob.running && nowSeconds - (inviteJob.completed_at ?? nowSeconds) >= 900 && <p className="muted">{t('The approval window closed.', 'Onay penceresi kapandı.')} <button className="text-button" disabled={busy} onClick={() => act({ kind: 'open_pairing', project_id: projectId })}>{t('Open approval window', 'Onay penceresini aç')}</button></p>}
    {card.grant_problem === 'REJECTED' && peer && <p className="muted">{t(`The invite service didn't accept ${peerName} (the project may already have 3 devices, or too many requests). Direct sharing still works.`, `Davet servisi ${named(collab, 'acc')} kabul etmedi (projede zaten 3 cihaz olabilir ya da çok fazla istek gelmiş olabilir). Doğrudan paylaşım çalışmaya devam eder.`)}</p>}

    {remoteAhead && peerVersion && <div className="new-version-notice" role="status">{mark(t(`New version from ${peerName}`, `${named(collab, 'abl')} yeni sürüm`), collab)} · {time(peerVersion.created_at)}</div>}
    {showOfflineHead && !offlineInProblem && <div className="new-version-notice" role="status">{mark(offlineLine, collab)}</div>}

    {peer && (resolveDivergence || resolveLocalChanges) && <section className="notice resolution-box" role="region" aria-label={t('Resolve project versions', 'Sürüm seçimi')}>
      <h3>{resolveLocalChanges && !resolveDivergence ? t("This folder has changes you haven't shared", 'Bu klasörde paylaşmadığın değişiklikler var') : t(`Your version and ${peerName}'s version both changed`, `Senin sürümün de ${named(collab, 'gen')} sürümü de değişti`)}</h3>
      <p>{t('TrackMesh does not merge projects. Choose which version to keep.', 'TrackMesh projeleri birleştirmez. Hangi sürümün kalacağını seç.')}</p>
      {!online && <p className="muted">{t(`Open TrackMesh on both PCs to use ${peerName}'s version.`, `${named(collab, 'gen')} sürümünü kullanmak için TrackMesh'i iki bilgisayarda da aç.`)}</p>}
      <div className="actions">
        <button disabled={busy || !online} onClick={() => void usePeerVersion()}>{t(`Use ${peerName}'s version`, `${named(collab, 'gen')} sürümünü kullan`)}</button>
        <button disabled={busy || rootMissing || project.recovery_required} onClick={() => { setKeepMinePending(true); void share(); }}>{t('Keep mine', 'Benimki kalsın')}</button>
      </div>
      {keepMinePending && job?.label === 'Share current version' && !job.running && !job.error && <p role="status">{t(`Ask ${peerName} to choose ‘Use ${status.device?.name ?? 'your device'}'s version’.`, `${named(collab, 'dat')} “${named(status.device?.name, 'gen')} sürümünü kullan” seçeneğini seçmesini söyle.`)}</p>}
    </section>}

    {peer && card.peers.length > 1 && <label className="peer-picker">{t('Collaborator', 'İş arkadaşı')} <select value={peerId ?? ''} disabled={busy} onChange={e => setSelected(e.target.value)}>
      {card.peers.map(p => <option key={p.device.device_id} value={p.device.device_id}>{p.device.name}</option>)}</select></label>}
    {/* Share and Get need the selected collaborator's approval; until then the card shows only the approval notice. */}
    {peer && pairingApproval !== 'PENDING' && pairingApproval !== 'NOT_REACHABLE' && <>
      <section className="share-flow">
        <div className="pane">
          <p className="muted">{mark(t(`Saves this folder as a new version ${peerName} can get.`, `Bu klasörü ${peerName} için yeni bir sürüm olarak kaydeder.`), collab)}</p>
          {firstShareBlocked
            ? <p className="muted">{card.root_empty && !peerHasVersion ? t('Add your project files to this folder first.', 'Önce proje dosyalarını bu klasöre ekle.') : t(`Get ${peerName}'s version first.`, `Önce ${named(collab, 'gen')} sürümünü al.`)}</p>
            : <button className={primary === 'share' && !resolveDivergence && !resolveLocalChanges ? 'primary' : ''} disabled={busy || removedHere || rootMissing || project.recovery_required} onClick={() => void share()}>{t('Share current version', 'Güncel sürümü paylaş')}</button>}
        </div>
        <div className="pane">
          {/* One line on what Get does now: the plugin's hand-off, a presence hint, up to date, or what it brings. */}
          {handoff ? <p className="handoff" role="status">{mark(t(`Save and close this project in your DAW, then press Get ${peerName}'s version.`, `Projeyi DAW'da kaydedip kapat, sonra “${named(collab, 'gen')} sürümünü al” düğmesine bas.`), collab)}</p>
            : getLine ? <p className="muted" role="status">{getLine}</p>
            : <p className="muted">{mark(online && peerEmpty ? t(`${peerName} has no version yet.`, `${named(collab, 'gen')} henüz sürümü yok.`)
              : upToDate ? t(`You have ${peerName}'s latest version`, `${named(collab, 'gen')} son sürümü sende`)
              : t(`Brings ${peerName}'s latest version into this folder.`, `${named(collab, 'gen')} son sürümünü bu klasöre getirir.`), collab)}</p>}
          <button className={primary === 'get' ? 'primary' : ''} disabled={disabledReceive} onClick={() => void receive()}>{resumable ? t('Resume getting', 'Almaya devam et') : t(`Get ${peerName}'s version`, `${named(collab, 'gen')} sürümünü al`)}</button>
        </div>
      </section></>}

    {!receiving && pairingApproval === 'NOT_REACHABLE' && peer && <section className="notice" role="status"><p>{t(`${peerName}'s approval of this device could not be confirmed. Ask them to keep TrackMesh open; if they haven't approved you yet, they can press ‘Open approval window’. Then press Ask for approval.`,
      `${named(collab, 'gen')} bu cihazı onayladığı doğrulanamadı. TrackMesh'i açık tutmasını söyle; seni henüz onaylamadıysa “Onay penceresini aç” düğmesine basabilir. Sonra Onay iste'ye bas.`)}</p>
      <div className="actions"><button disabled={busy} onClick={() => act({ kind: 'request_pairing', project_id: projectId, device_id: peer.device.device_id })}>{t('Ask for approval', 'Onay iste')}</button></div></section>}
    {statusLine}

    {removing !== null && <section className="notice warn remove-box" role="region" aria-label={t('Remove from TrackMesh', "TrackMesh'ten kaldır")}>
      <strong>{leaving ? t(`Leave ${project.name}`, `${project.name} projesinden ayrıl`) : t(`Remove ${project.name} from TrackMesh?`, `${project.name} TrackMesh'ten kaldırılsın mı?`)}</strong>
      <p>{t('The folder and its files stay as they are.', 'Klasör ve içindeki dosyalar olduğu gibi kalır.')}{' '}
        {!peer ? '' : creator ? mark(t(`${peerName} can no longer get its versions.`, `${peerName} artık bu projenin sürümlerini alamaz.`), collab)
          : mark(t(`This PC stops sharing with ${peerName}.`, `Bu bilgisayar ${named(collab, 'abl')} sürüm almayı ve göndermeyi bırakır.`), collab)}</p>
      <label className="check"><input type="checkbox" checked={removing} onChange={event => setRemoving(event.target.checked)} />
        {t("Also delete this PC's version history and the project key", 'Bu bilgisayardaki sürüm geçmişini ve proje anahtarını da sil')}</label>
      <p className="muted">{removing ? t('This cannot be undone; coming back needs a new invite.', 'Bu geri alınamaz; projeye dönmek için yeni bir davet gerekir.')
        : t('You can bring it back later in Settings, Removed projects.', "Sonra Ayarlar'daki Kaldırılan projeler bölümünden geri getirebilirsin.")}</p>
      <div className="actions"><button className="danger" disabled={busy} onClick={async () => { if (await act({ kind: 'remove_project', project_id: projectId, delete_data: removing })) setRemoving(null); }}>{leaving ? t('Leave project', 'Projeden ayrıl') : t('Remove', 'Kaldır')}</button>
        <button onClick={() => setRemoving(null)}>{t('Cancel', 'Vazgeç')}</button></div>
    </section>}

    <footer className="card-footer">
      {/* The creator removes a collaborator here; a member leaves instead. */}
      {peer && creator && <button className="text-button" disabled={busy} onClick={async () => await ask(t('Remove access?', 'Erişim kaldırılsın mı?'),
        t(`${peerName} will no longer get versions of ${project.name}.\n\nThey keep the files they already received and this project's key. The key doesn't let them get new versions; if they ask to join again while an invite is open, approve only if you want them back.`, `${peerName} artık ${project.name} sürümlerini alamayacak.\n\nDaha önce aldığı dosyalar ve bu projenin anahtarı onda kalır. Anahtarla yeni sürüm alamaz; bir davet açıkken yeniden katılmak isterse yalnızca geri istiyorsan onayla.`),
        t('Remove access', 'Erişimi kaldır'), t('Cancel', 'Vazgeç')).catch(() => false) && act({ kind: 'revoke', project_id: projectId, device_id: peer.device.device_id })}>{t(`Remove ${peerName}`, `${named(collab, 'acc')} çıkar`)}</button>}
      {/* A removed member already has this action in its notice. */}
      {!removedHere && <button className="text-button" disabled={busy || removing !== null} onClick={() => setRemoving(false)}>{leaving ? t('Leave project', 'Projeden ayrıl') : t('Remove from TrackMesh', "TrackMesh'ten kaldır")}</button>}
      {peer && offerButtons}
      {peer && creator && <button className="text-button" disabled={busy || !inviteReady} title={inviteReady ? t('Up to 3 devices per project', 'Proje başına en fazla 3 cihaz') : t('Needs the invite service', 'Davet servisi gerekir')} onClick={() => act({ kind: 'copy_invite_link', project_id: projectId })}>{t('Invite another collaborator', 'Başka birini davet et')}</button>}
      <button className="text-button" onClick={() => openAdvanced(projectId, 'connection')}>{t('Advanced →', 'Gelişmiş →')}</button>
    </footer>
  </article>;
}
