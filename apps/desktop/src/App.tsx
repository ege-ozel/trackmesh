import { useCallback, useEffect, useRef, useState } from 'react';
import { AdvancedScreen } from './AdvancedScreen';
import { cancelInviteLink, chooseFolder, chooseInviteParentFolder, closeApp, getStatus, openInviteParentFolder, pasteInviteLink, setLanguage, setUpnpMapping, submitAction } from './api';
import type { Action, DesktopStatus } from './api';
import { CloseButton, JobNotice, cancelled, friendlyError } from './Jobs';
import { Card, ProjectTabs } from './ProjectCard';
import type { AdvancedSection } from './ProjectCard';
import { DeviceName, SettingsScreen, ZOOM_LEVELS } from './SettingsScreen';
import { displayPath } from './format';
import { readLang, saveLang, setLang, t } from './i18n';
import type { Lang } from './i18n';

type Screen = 'projects' | 'advanced' | 'settings';

// A saved setting shows in its own control; only its errors need the line at the top.
const QUIET_SETTINGS = new Set(['Save DAW auto-share setting', 'Save device name', 'Save Projects location', 'Save default Supabase settings', 'Save manual port forwarding']);

const screenTitles = (): Record<Screen, [string, string]> => ({
  projects: [t('Projects', 'Projeler'), t('The DAW project folders you share with your collaborator.', 'İş arkadaşınla paylaştığın DAW proje klasörleri.')],
  advanced: [t('Advanced', 'Gelişmiş'), t('Connection, invite service and version details. TrackMesh sends you here when something needs fixing.', 'Bağlantı, davet servisi ve sürüm ayrıntıları. Bir şeyin düzeltilmesi gerektiğinde TrackMesh seni buraya getirir.')],
  settings: [t('Settings', 'Ayarlar'), t('Your name, sharing, language and storage.', 'Adın, paylaşım, dil ve depolama.')],
});

function readZoom() {
  try {
    const value = Number(window.localStorage.getItem('trackmesh.zoom'));
    return ZOOM_LEVELS.some(level => level.value === value) ? value : 1;
  } catch { return 1; }
}

function readProject() {
  try { return window.localStorage.getItem('trackmesh.project'); } catch { return null; }
}

export default function App() {
  const [status, setStatus] = useState<DesktopStatus | null>(null);
  const [error, setError] = useState('');
  // Asked once per app start: the firewall is observed when TrackMesh starts.
  const [publicNetworkOk, setPublicNetworkOk] = useState(false);
  const [offset, setOffset] = useState(0);
  const [setup, setSetup] = useState<'link' | 'join' | null>(null);
  const [screen, setScreen] = useState<Screen>('projects');
  // One selected project for both Projects and Advanced, remembered per viewer.
  const [selectedProjectId, setSelectedProjectId] = useState<string | null>(readProject);
  const [advancedSection, setAdvancedSection] = useState<AdvancedSection | null>(null);
  const [zoom, setZoom] = useState(readZoom);
  const [lang, setLangState] = useState<Lang>(readLang);
  setLang(lang);
  const [folder, setFolder] = useState('');
  const [name, setName] = useState('');
  const [autoName, setAutoName] = useState(false);
  const [inviteServiceUrl, setInviteServiceUrl] = useState('');
  const [inviteServiceKey, setInviteServiceKey] = useState('');
  const [inviteParentFolder, setInviteParentFolder] = useState<string | null>(null);
  const [jobStartedAt, setJobStartedAt] = useState<Record<string, number>>({});
  const [focusProjectId, setFocusProjectId] = useState<string | null>(null);
  const [scrolledFocusId, setScrolledFocusId] = useState<string | null>(null);
  const pluginHandoffRevision = useRef(0);
  useEffect(() => {
    const handoff = status?.plugin_handoff;
    if (!handoff || handoff.revision === pluginHandoffRevision.current) return;
    pluginHandoffRevision.current = handoff.revision;
    focusBaseline.current = null;
    setScreen('projects');
    setSetup(null);
    setOffset(handoff.offset);
    setScrolledFocusId(null);
    setFocusProjectId(handoff.project_id);
    setSelectedProjectId(handoff.project_id);
  }, [status?.plugin_handoff]);
  const [upnpConsentPrompt, setUpnpConsentPrompt] = useState(false);
  const pendingConsentAction = useRef<Action | null>(null);
  const polling = useRef(false);
  const mounted = useRef(true);
  const generation = useRef(0);
  const submitted = useRef(false);
  const focusBaseline = useRef<Set<string> | null>(null);
  const poll = useCallback(async () => {
    if (polling.current) return;
    polling.current = true;
    const request = generation.current;
    try { const value = await getStatus(offset); if (mounted.current && generation.current === request) { setStatus(value); } }
    catch (err) { if (mounted.current) setError(String(err)); }
    finally { polling.current = false; }
  }, [offset]);
  useEffect(() => {
    mounted.current = true;
    generation.current += 1;
    void poll();
    const interval = window.setInterval(() => void poll(), 1500);
    return () => { mounted.current = false; generation.current += 1; window.clearInterval(interval); };
  }, [poll]);
  useEffect(() => {
    if (!status) return;
    setJobStartedAt(current => {
      const next: Record<string, number> = {};
      const now = Date.now();
      for (const job of status.jobs) {
        next[job.job_id] = current[job.job_id] ?? now;
      }
      return next;
    });
  }, [status?.jobs]);
  // Text size: the root zoom keeps every spacing and font in proportion.
  useEffect(() => {
    document.documentElement.style.zoom = String(zoom);
    try { window.localStorage.setItem('trackmesh.zoom', String(zoom)); } catch { /* per-viewer convenience only */ }
  }, [zoom]);
  useEffect(() => {
    document.documentElement.lang = lang;
    saveLang(lang);
    void setLanguage(lang).catch(() => { /* only the native quit dialog's language */ });
  }, [lang]);
  useEffect(() => {
    if (!selectedProjectId) return;
    try { window.localStorage.setItem('trackmesh.project', selectedProjectId); } catch { /* per-viewer convenience only */ }
  }, [selectedProjectId]);
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (!event.ctrlKey || event.altKey) return;
      const index = ZOOM_LEVELS.findIndex(level => level.value === zoom);
      if (event.key === '+' || event.key === '=') setZoom(ZOOM_LEVELS[Math.min(ZOOM_LEVELS.length - 1, index + 1)].value);
      else if (event.key === '-') setZoom(ZOOM_LEVELS[Math.max(0, index - 1)].value);
      else if (event.key === '0') setZoom(1);
      else return;
      event.preventDefault();
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [zoom]);
  useEffect(() => {
    if (!status?.page || !focusBaseline.current) return;
    const added = status.page.projects.find(item => !focusBaseline.current?.has(item.project.project_id));
    if (added) {
      focusBaseline.current = null;
      setScrolledFocusId(null);
      setFocusProjectId(added.project.project_id);
      setSelectedProjectId(added.project.project_id);
    }
  }, [status?.page]);
  // Scroll once per focus request; status polling must not re-scroll or restart the highlight timer.
  useEffect(() => {
    if (!focusProjectId || scrolledFocusId === focusProjectId) return;
    const frame = window.requestAnimationFrame(() => {
      const card = document.getElementById(`project-${focusProjectId}`);
      if (!card) return;
      card.scrollIntoView({ behavior: 'smooth', block: 'nearest' });
      setScrolledFocusId(focusProjectId);
    });
    return () => window.cancelAnimationFrame(frame);
  }, [focusProjectId, scrolledFocusId, status?.page, screen]);
  useEffect(() => {
    if (!scrolledFocusId) return;
    const timeout = window.setTimeout(() => { setFocusProjectId(null); setScrolledFocusId(null); }, 2500);
    return () => window.clearTimeout(timeout);
  }, [scrolledFocusId]);
  useEffect(() => {
    if (!advancedSection) return;
    const timeout = window.setTimeout(() => setAdvancedSection(null), 2500);
    return () => window.clearTimeout(timeout);
  }, [advancedSection, selectedProjectId]);
  const executeAction = async (action: Action): Promise<boolean> => {
    if (submitted.current) return false;
    submitted.current = true;
    setError('');
    try { await submitAction(action); await poll(); return true; }
    catch (err) { setError(String(err)); return false; }
    finally { submitted.current = false; }
  };
  const act = async (action: Action): Promise<boolean> => {
    const asksForConsent = action.kind === 'copy_invite_link' || action.kind === 'join' || action.kind === 'complete_invite_link';
    if (asksForConsent && status && !status.upnp_mapping_asked) {
      pendingConsentAction.current = action;
      setUpnpConsentPrompt(true);
      return true;
    }
    return executeAction(action);
  };
  const answerUpnpConsent = async (allow: boolean) => {
    const action = pendingConsentAction.current;
    pendingConsentAction.current = null;
    setUpnpConsentPrompt(false);
    try { await setUpnpMapping(allow); await poll(); }
    catch (err) { setError(`${t('Could not save router preference:', 'Modem tercihi kaydedilemedi:')} ${String(err)}`); }
    if (action) await executeAction(action);
  };
  const go = (next: Screen) => { setScreen(next); setAdvancedSection(null); window.scrollTo({ top: 0 }); };
  const openAdvanced = (projectId: string, section: AdvancedSection) => {
    setSelectedProjectId(projectId);
    setAdvancedSection(section);
    setScreen('advanced');
    window.scrollTo({ top: 0 });
  };
  const startLink = () => { go('projects'); setSetup('link'); setFolder(''); setName(''); setAutoName(false); };
  const pasteInvite = async () => {
    setSetup(null);
    setScreen('projects');
    setOffset(0);
    focusBaseline.current = new Set(status?.page?.projects.map(item => item.project.project_id) ?? []);
    try { await pasteInviteLink(); setError(''); await poll(); } catch (err) { setError(String(err)); }
  };
  const busy = !status?.ready || status.closing || status.jobs.some(j => j.running && !j.project_id);
  const browse = async () => {
    try {
      const value = await chooseFolder();
      if (value) {
        setFolder(value);
        const base = value.replace(/[\\/]+$/, '').split(/[\\/]/).pop() ?? '';
        if (setup === 'link' && (!name.trim() || autoName)) { setName(base); setAutoName(true); }
      }
    } catch (err) { setError(String(err)); }
  };
  const saveSetup = async () => {
    if (setup === 'join') {
      setOffset(0);
      focusBaseline.current = new Set(status?.page?.projects.map(item => item.project.project_id) ?? []);
      if (await act({ kind: 'join', folder })) setSetup(null);
    } else {
      // Each project gets a new permanent ID; the collaborator receives it with the invite.
      const projectId = crypto.randomUUID();
      setOffset(0);
      focusBaseline.current = null;
      if (await act({ kind: 'link', project_id: projectId, name: name.trim(), folder })) {
        setSetup(null);
        setScrolledFocusId(null);
        setFocusProjectId(projectId);
        setSelectedProjectId(projectId);
      }
    }
  };
  const projects = status?.page?.projects ?? [];
  const shownProject = projects.find(item => item.project.project_id === selectedProjectId) ?? projects[0];
  // Running work plus the latest result while it is fresh: successes for 20 s, errors for 2 min.
  const nowSeconds = Math.floor(Date.now() / 1000);
  const projectlessJobs = (status?.jobs.filter(j => !j.project_id && !(QUIET_SETTINGS.has(j.label) && !j.error)) ?? []).filter((job, _index, all) => job.running
    || (job === all.filter(item => !item.running).slice(-1)[0] && nowSeconds - (job.completed_at ?? nowSeconds) < (job.error ? 120 : 20)));
  const firstRun = !!status?.ready && !projects.length && !setup && offset === 0;
  // A name nobody chose (the old default or a Windows computer name) reads badly on the other PC, e.g.
  // "a new version DESKTOP-AB12CD3 can get". Ask for one until it is changed.
  const deviceName = status?.device?.name ?? '';
  const unnamedDevice = !!status?.ready && (deviceName === 'My studio PC' || /^(DESKTOP|LAPTOP|PC|WIN)-[A-Z0-9]{4,}$/i.test(deviceName));
  const [title, subtitle] = screenTitles()[screen];
  // Errors stay until closed; a closed backend error comes back only if its text changes.
  const [closedError, setClosedError] = useState('');
  const shownError = error || status?.error || status?.network?.error || '';
  const runtimeError = cancelled(shownError) || shownError === closedError ? '' : shownError;
  const nav = (target: Screen, label: string) =>
    <button className={screen === target ? 'rail-nav active' : 'rail-nav'} aria-current={screen === target ? 'page' : undefined} onClick={() => go(target)}>{label}</button>;

  return <div className="app-shell">
    <aside className="rail" aria-label="Main">
      <div className="brand"><span className="brand-mark">TM</span><span className="wordmark">TrackMesh</span></div>
      {nav('projects', t('Projects', 'Projeler'))}
      {nav('advanced', t('Advanced', 'Gelişmiş'))}
      {nav('settings', t('Settings', 'Ayarlar'))}
      <div className="rail-bottom">
        <div className="device"><span className={`status-dot ${status?.ready ? 'active' : ''}`} /><span>{status?.device?.name ?? t('Starting…', 'Başlatılıyor…')}</span></div>
        <button title={t('Confirm who will lose access to your versions, finish accepted work, then quit TrackMesh. Closing the window keeps it in the system tray.', 'Sürümlerine kimin erişemeyeceğini onayla, süren işler bitsin, sonra TrackMesh kapansın. Pencereyi kapatmak onu saatin yanında açık tutar.')} disabled={status?.closing} onClick={() => void closeApp().catch(err => setError(String(err)))}>{t('Quit TrackMesh', "TrackMesh'ten çık")}</button>
      </div>
    </aside>
    <main>
      <header className="page-head">
        <div><h1>{firstRun && screen === 'projects' ? t('Share a DAW project with one collaborator', 'Bir DAW projesini iş arkadaşınla paylaş') : title}</h1>
          <p className="muted">{firstRun && screen === 'projects' ? t('Files go directly between your PCs. Nothing is uploaded to a cloud drive.', 'Dosyalar doğrudan bilgisayarlarınız arasında gider. Hiçbir şey buluta yüklenmez.') : subtitle}</p></div>
        {screen === 'projects' && !firstRun && <div className="actions">
          <button disabled={busy} onClick={startLink}>{t('Add project', 'Proje ekle')}</button>
          <button disabled={busy} onClick={() => void pasteInvite()}>{t('Paste invite link', 'Davet bağlantısını yapıştır')}</button></div>}
        {screen === 'advanced' && <button className="text-button" onClick={() => go('projects')}>{t('← Back to Projects', '← Projelere dön')}</button>}
      </header>
      {runtimeError && <div className="notice danger closable" role="alert"><CloseButton onClose={() => { setError(''); setClosedError(runtimeError); }} />{friendlyError(runtimeError) ?? runtimeError}</div>}
      {!publicNetworkOk && !status?.closing && status?.firewall_observation.profiles.some(profile => profile.name === 'PUBLIC') && <section className="notice danger" role="alert">
        <strong>{t('This PC is on a network Windows marks as Public.', "Bu bilgisayar Windows'un Public (genel) olarak işaretlediği bir ağda.")}</strong>
        <p>{t(`TrackMesh accepts connections on port ${status.listen_port} here too, so anyone on this network (café, hotel, campus Wi-Fi) can reach it. Without your approval they can't get your files or join a project. They can see that TrackMesh runs on this PC and try to slow it down or crash it, and an unknown bug in TrackMesh or its network code could let them do more.`,
          `TrackMesh burada da ${status.listen_port} portundan bağlantı kabul eder; bu ağdaki herkes (kafe, otel, kampüs Wi-Fi'si) ona ulaşabilir. Senin onayın olmadan dosyalarını alamaz, projeye katılamazlar. Bu bilgisayarda TrackMesh çalıştığını görebilir, onu yavaşlatmayı ya da çökertmeyi deneyebilirler; TrackMesh'te veya ağ kodunda bilinmeyen bir hata daha fazlasına izin verebilir.`)}</p>
        <p>{t('If this is your home network, set it to Private in Windows Settings → Network & internet, then restart TrackMesh.', "Bu senin ev ağınsa Windows Ayarları → Ağ ve internet bölümünden Özel (Private) yap, sonra TrackMesh'i yeniden başlat.")}</p>
        <p><strong>{t('Use TrackMesh on this network?', 'TrackMesh bu ağda kullanılsın mı?')}</strong></p>
        <div className="actions"><button onClick={() => setPublicNetworkOk(true)}>{t('Use on this network', 'Bu ağda kullan')}</button>
          <button className="primary" onClick={() => void closeApp().catch(err => setError(String(err)))}>{t('Quit TrackMesh', "TrackMesh'ten çık")}</button></div>
      </section>}
      {status?.closing && <div className="notice">{t('Work finished. TrackMesh is shutting down safely.', 'İşler bitti. TrackMesh güvenle kapanıyor.')}</div>}
      {/* Projectless results (invite link, name, settings) sit at the top so they are seen on any page. */}
      {projectlessJobs.filter(job => job.label !== 'Accept invite link' || !status?.invite_pending).map(job => <JobNotice key={job.job_id} job={job} startedAt={jobStartedAt[job.job_id]} />)}
      {/* A device this PC already works with offers another project; no link and no approval wait. */}
      {screen === 'projects' && status?.pairing.offers.map(item => <section key={item.project_id} className="approval-box" role="region" aria-label={t('Project offer', 'Proje teklifi')}>
        <h3>{t(`${item.name} wants to share ${item.project_name} with you`, `${item.name} seninle ${item.project_name} projesini paylaşmak istiyor`)}</h3>
        <p className="muted">{item.existing_root
          ? <>{t('It goes back to its old folder', 'Eski klasörüne geri döner:')} {displayPath(item.existing_root)}</>
          : <>{t('The project will be saved in', 'Proje şuraya kaydedilecek:')} {displayPath(status.projects_location)}\{item.project_name}</>}</p>
        <div className="actions"><button className="primary" disabled={busy} onClick={() => { setOffset(0); focusBaseline.current = new Set(projects.map(project => project.project.project_id)); void act({ kind: 'accept_offer', project_id: item.project_id }); }}>{t('Accept', 'Kabul et')}</button>
          <button disabled={busy} onClick={() => void act({ kind: 'decline_offer', project_id: item.project_id })}>{t('Decline', 'Reddet')}</button></div></section>)}

      {status?.invite_pending && <div className="invite-overlay"><section className="invite-dialog" role="dialog" aria-modal="true" aria-labelledby="invite-title"><h2 id="invite-title">{t('Invite link', 'Davet bağlantısı')}</h2>
        {status.jobs.filter(job => job.label === 'Accept invite link' && job.error).slice(-1).map(job => <div className="notice danger" role="alert" key={job.job_id}>{/Invite link is invalid, expired, or unavailable/i.test(job.error ?? '')
          ? <><strong>{t('This link has expired, was already used, or could not be checked.', 'Bu bağlantının süresi dolmuş, bağlantı kullanılmış ya da kontrol edilemedi.')}</strong><p>{t('Ask for a new link, or Cancel.', 'Yeni bir bağlantı iste ya da Vazgeç.')}</p></>
          : <strong>{friendlyError(job.error ?? '') ?? job.error}</strong>}</div>)}
        <p>{t('The project will be saved in', 'Proje şuraya kaydedilecek:')} <button className="path-link" title={t('Open in Explorer', "Gezgin'de aç")} onClick={() => void openInviteParentFolder().catch(err => setError(String(err)))}>{displayPath(inviteParentFolder ?? status.projects_location)}\[{t('project name', 'proje adı')}]</button>{t('.', '')}</p>
        {status.invite_needs_service_fields && <><label className="field">Supabase Project URL<input autoComplete="off" value={inviteServiceUrl} onChange={event => setInviteServiceUrl(event.target.value)} placeholder="https://your-project.supabase.co" /></label>
        <label className="field">Publishable key<input type="password" autoComplete="new-password" value={inviteServiceKey} onChange={event => setInviteServiceKey(event.target.value)} /></label></>}
        {/* The device request carries this name, so the sender approves a name they recognise. */}
        <DeviceName name={deviceName} busy={busy} act={act} hint={t('The sender sees this name when approving you.', 'Gönderen seni onaylarken bu adı görür.')} />
        <p className="muted">{t('After you accept, the sender approves you and the project downloads by itself.', 'Kabul edince gönderen seni onaylar ve proje kendiliğinden iner.')}</p>
        <div className="actions">
          <button disabled={busy} onClick={async () => { try { const value = await chooseInviteParentFolder(); if (value) setInviteParentFolder(value); } catch (err) { setError(String(err)); } }}>{t('Choose another folder…', 'Başka klasör seç…')}</button>
          <span className="spacer" />
          <button disabled={!status.ready || status.closing || status.jobs.some(job => job.running && job.label === 'Accept invite link')} onClick={() => void cancelInviteLink().then(poll).catch(err => setError(String(err)))}>{t('Cancel', 'Vazgeç')}</button>
          <button className="primary" disabled={busy || (status.invite_needs_service_fields && (!inviteServiceUrl.trim() || !inviteServiceKey.trim()))} onClick={() => {
            setSetup(null); setScreen('projects'); setOffset(0);
            focusBaseline.current = new Set(status.page?.projects.map(item => item.project.project_id) ?? []);
            void act({ kind: 'complete_invite_link', ...(status.invite_needs_service_fields ? { supabase_url: inviteServiceUrl.trim(), supabase_anon_key: inviteServiceKey.trim() } : {}) });
          }}>{t('Accept', 'Kabul et')}</button></div>
      </section></div>}
      {upnpConsentPrompt && <div className="invite-overlay"><section className="invite-dialog upnp-consent-dialog" role="dialog" aria-modal="true" aria-labelledby="upnp-consent-title"><h2 id="upnp-consent-title">{t('Router setup', 'Modem ayarı')}</h2>
        <p>{t('Let TrackMesh set up your router automatically? Needed when your collaborator is on a different internet connection. Opens one port only for TrackMesh; only paired devices can use it. You can change this later in Settings.',
          "TrackMesh modemini kendisi ayarlasın mı? İş arkadaşın farklı bir internet bağlantısındaysa gerekir. Yalnızca TrackMesh için bir port açar; onu yalnızca eşleşen cihazlar kullanabilir. Bunu sonra Ayarlar'dan değiştirebilirsin.")}</p>
        <div className="actions"><span className="spacer" /><button onClick={() => void answerUpnpConsent(false)}>{t('Not now', 'Şimdi değil')}</button><button className="primary" onClick={() => void answerUpnpConsent(true)}>{t('Allow (recommended)', 'İzin ver (önerilen)')}</button></div>
      </section></div>}

      {screen === 'settings' && <SettingsScreen status={status} busy={busy} act={act} poll={poll} onError={setError} zoom={zoom} setZoom={setZoom} lang={lang} setLang={setLangState} />}
      {screen === 'advanced' && status && <AdvancedScreen status={status} projectId={shownProject?.project.project_id ?? null} section={advancedSection} jobStartedAt={jobStartedAt} act={act}
        pickProject={projectId => { setSelectedProjectId(projectId); setAdvancedSection(null); }}
        openInvitationFile={() => { go('projects'); setSetup('join'); setFolder(''); }} onError={setError} />}
      {screen === 'projects' && <>
        {setup && <section className="setup-panel"><h2>{setup === 'join' ? t('Open an invitation file', 'Davet dosyası aç') : t('Add project', 'Proje ekle')}</h2>
          {setup === 'link' && <label className="field">{t('Project name', 'Proje adı')}<input value={name} maxLength={320} onChange={e => { setName(e.target.value); setAutoName(false); }} placeholder="Track 07" /></label>}
          <label className="field">{setup === 'join' ? t('Empty folder for the project', 'Proje için boş klasör') : t('Folder with your Ableton or FL project', 'Ableton ya da FL projenin klasörü')}<div className="input-row"><input value={folder} onChange={e => setFolder(e.target.value)} placeholder="D:\Music\Track 07" /><button onClick={() => void browse()}>{t('Browse…', 'Gözat…')}</button></div></label>
          {setup === 'join' && <p className="muted">{t('Choose the folder, then open the private invitation file. Your device request is sent automatically; the sender keeps TrackMesh open and approves you.', "Klasörü seç, sonra özel davet dosyasını aç. Cihaz isteğin kendiliğinden gider; gönderen TrackMesh'i açık tutar ve seni onaylar.")}</p>}
          {setup === 'link' && <p className="muted">{t('Next you invite your collaborator from the project card.', 'Sonra iş arkadaşını proje kartından davet edersin.')}</p>}
          <div className="actions"><button className="primary" disabled={busy || !folder || (setup === 'link' && !name.trim())} onClick={() => void saveSetup()}>{setup === 'join' ? t('Open invitation file…', 'Davet dosyasını aç…') : t('Add project', 'Projeyi ekle')}</button><button onClick={() => setSetup(null)}>{t('Cancel', 'Vazgeç')}</button></div>
        </section>}
        {!firstRun && status?.ready && <section className={unnamedDevice ? 'notice' : 'device-strip'}>
          <DeviceName name={deviceName} busy={busy} act={act} collapsible={!unnamedDevice} hint={t('Give this PC a name your collaborators will recognise. New invites use it; collaborators who already approved this PC keep seeing the old name for now.',
            'Bu bilgisayara iş arkadaşlarının tanıyacağı bir ad ver. Yeni davetler bu adı kullanır; bu bilgisayarı önceden onaylamış iş arkadaşların şimdilik eski adı görmeye devam eder.')} /></section>}
        {firstRun && <section className="first-run">
          <DeviceName name={deviceName} busy={busy} act={act} hint={t('Your collaborator sees this name. You can change it later in Settings.', "İş arkadaşın bu adı görür. Sonra Ayarlar'dan değiştirebilirsin.")} />
          <ol className="first-steps">
            <li><strong>{t('Pick the project folder', 'Proje klasörünü seç')}</strong><span>{t('The folder that holds your Ableton or FL project.', 'Ableton ya da FL projenin olduğu klasör.')}</span></li>
            <li><strong>{t('Invite and approve', 'Davet et ve onayla')}</strong><span>{t('Send a link, then approve your collaborator.', 'Bir bağlantı gönder, sonra iş arkadaşını onayla.')}</span></li>
            <li><strong>{t('Share and get versions', 'Sürüm paylaş ve al')}</strong><span>{t('One button each way.', 'Her yön için tek düğme.')}</span></li>
          </ol>
          <div className="choices">
            <section className="choice"><span className="who">{t('I have the project', 'Proje bende')}</span><h2>{t('Add project', 'Proje ekle')}</h2>
              <p>{t('Choose the folder with your DAW project. TrackMesh keeps its versions and lets you invite a collaborator.', 'DAW projenin klasörünü seç. TrackMesh sürümlerini tutar ve iş arkadaşını davet etmeni sağlar.')}</p>
              <button className="primary" disabled={busy} onClick={startLink}>{t('Choose folder…', 'Klasör seç…')}</button></section>
            <section className="choice"><span className="who">{t('Someone sent me a link', 'Bana bir bağlantı gönderildi')}</span><h2>{t('Invite link', 'Davet bağlantısı')}</h2>
              <p>{t('Copy the link you received, then press the button. The project downloads after the sender approves you.', 'Gelen bağlantıyı kopyala, sonra düğmeye bas. Gönderen seni onaylayınca proje iner.')}</p>
              <button disabled={busy} onClick={() => void pasteInvite()}>{t('Paste invite link', 'Davet bağlantısını yapıştır')}</button></section>
          </div>
          <p className="muted">{t('Closing the window keeps TrackMesh running next to the clock, so your collaborator can still get your versions.', 'Pencereyi kapatınca TrackMesh saatin yanında çalışmaya devam eder; iş arkadaşın sürümlerini almaya devam edebilir.')}</p>
        </section>}
        {/* One project at a time; the tabs appear once there is more than one. */}
        {status && projects.length > 1 && shownProject && <ProjectTabs projects={projects} status={status} selectedId={shownProject.project.project_id} pick={setSelectedProjectId} />}
        {status && shownProject && <Card key={shownProject.project.project_id} card={shownProject} status={status} jobStartedAt={jobStartedAt} focusProjectId={focusProjectId} act={act} openAdvanced={openAdvanced} />}
        {(offset > 0 || status?.page?.has_more) && <div className="pagination"><button disabled={!offset || busy} onClick={() => setOffset(Math.max(0, offset - 32))}>{t('Previous', 'Önceki')}</button><span className="muted">{t('Page', 'Sayfa')} {offset / 32 + 1}</span><button disabled={!status?.page?.has_more || busy} onClick={() => setOffset(offset + 32)}>{t('Next', 'Sonraki')}</button></div>}
      </>}
    </main>
  </div>;
}
