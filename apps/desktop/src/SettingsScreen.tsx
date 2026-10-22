import { useEffect, useState } from 'react';
import { chooseProjectsLocation, openProjectsLocation, setUpnpMapping } from './api';
import type { Action, DesktopStatus } from './api';
import { StorageSettings } from './StorageSettings';
import { displayPath } from './format';
import { LANGUAGES, t } from './i18n';
import type { Lang } from './i18n';

type Act = (action: Action) => Promise<boolean>;

export const ZOOM_LEVELS = [{ value: 1, label: 'Normal' }, { value: 1.15, label: 'Large' }, { value: 1.3, label: 'Larger' }] as const;
const zoomLabel = (label: string) => label === 'Large' ? t('Large', 'Büyük') : label === 'Larger' ? t('Larger', 'Daha büyük') : t('Normal', 'Normal');

/** `collapsible`: a chosen name shows as one line with Edit; the form opens with an ease and closes on Save or Cancel. */
export function DeviceName({ name, busy, act, hint, collapsible }: { name: string; busy: boolean; act: Act; hint?: string; collapsible?: boolean }) {
  const [value, setValue] = useState(name);
  const [saved, setSaved] = useState(false);
  const [editing, setEditing] = useState(false);
  useEffect(() => setValue(name), [name]);
  if (collapsible) return <div className={editing ? 'device-name open' : 'device-name'}>
    <div className="device-line"><span className="muted">{t('Your name', 'Adın')}</span><strong>{name}</strong>
      {!editing && <button className="text-button" onClick={() => setEditing(true)}>{t('Edit', 'Düzenle')}</button>}</div>
    <div className="expand" inert={!editing}><div><div className="input-row"><input maxLength={80} value={value} disabled={busy} aria-label={t('Your name', 'Adın')} onChange={event => setValue(event.target.value)} />
      <button disabled={busy || !value.trim() || value.trim() === name} onClick={async () => { if (await act({ kind: 'set_device_name', name: value.trim() })) setEditing(false); }}>{t('Save', 'Kaydet')}</button>
      <button onClick={() => { setValue(name); setEditing(false); }}>{t('Cancel', 'Vazgeç')}</button></div>
      {hint && <p className="muted">{hint}</p>}</div></div>
  </div>;
  return <section className="settings-block"><h3>{t('Your name', 'Adın')}</h3>
    <label className="field">{t('Shown to your collaborators', 'İş arkadaşlarına görünür')}<div className="input-row"><input maxLength={80} value={value} disabled={busy} onChange={event => { setValue(event.target.value); setSaved(false); }} />
      <button disabled={busy || !value.trim() || value.trim() === name} onClick={async () => setSaved(await act({ kind: 'set_device_name', name: value.trim() }))}>{t('Save', 'Kaydet')}</button></div></label>
    {saved && value.trim() === name && <p className="saved-note" role="status">{t('Saved ✓', 'Kaydedildi ✓')}</p>}
    {hint && <p className="muted">{hint}</p>}
  </section>;
}

export function SettingsScreen({ status, busy, act, poll, onError, zoom, setZoom, lang, setLang }: {
  status: DesktopStatus | null; busy: boolean; act: Act; poll: () => Promise<void>; onError: (message: string) => void;
  zoom: number; setZoom: (value: number) => void; lang: Lang; setLang: (value: Lang) => void;
}) {
  const [projectsLocation, setProjectsLocation] = useState('');
  useEffect(() => setProjectsLocation(status?.projects_location ?? ''), [status?.projects_location]);
  const upnp = status?.network?.port_mapping;
  return <section className="settings-screen">
    <DeviceName name={status?.device?.name ?? ''} busy={busy} act={act} />

    <section className="settings-block"><h3>{t('Share when I save in the DAW', "DAW'da kaydedince paylaş")}</h3>
      <p className="muted">{t('With the TrackMesh plugin in the project, saving in the DAW shares a new version.', "Projede TrackMesh eklentisi varsa, DAW'da kaydedince yeni bir sürüm paylaşılır.")}</p>
      {status?.daw_plugin_settings.map(project => <div className="toggle-row" key={project.project_id}>
        <div><strong>{project.name}</strong></div>
        <label className="switch"><input type="checkbox" checked={project.auto_share} disabled={busy || !status.ready} onChange={event => void act({ kind: 'set_auto_share', project_id: project.project_id, enabled: event.currentTarget.checked })} /> {t('Share on save', 'Kaydedince paylaş')}</label>
      </div>)}
      {status?.ready && !status.daw_plugin_settings.length && <p className="muted">{t('Add a project first.', 'Önce bir proje ekle.')}</p>}
    </section>

    {!!status?.removed_projects.length && <section className="settings-block"><h3>{t('Removed projects', 'Kaldırılan projeler')}</h3>
      <p className="muted">{t('Removed with their versions kept. Bringing one back does not bring back its collaborators; invite them again.', 'Sürümleri saklanarak kaldırıldı. Geri getirmek iş arkadaşlarını geri getirmez; onları yeniden davet et.')}</p>
      {status.removed_projects.map(project => <div className="toggle-row" key={project.project_id}>
        <div><strong>{project.name}</strong><p className="muted">{project.root}</p></div>
        <button disabled={busy} onClick={() => void act({ kind: 'restore_project', project_id: project.project_id })}>{t('Bring back', 'Geri getir')}</button>
      </div>)}
    </section>}

    <section className="settings-block"><h3>{t('Connection', 'Bağlantı')}</h3>
      <div className="switch-row"><label className="switch"><input type="checkbox" checked={upnp?.consent_enabled ?? false} disabled={busy} onChange={event => { void setUpnpMapping(event.currentTarget.checked).then(poll).catch(err => onError(String(err))); }} /> {t('Set up my router automatically (UPnP)', 'Modemimi kendisi ayarlasın (UPnP)')}</label>
        {upnp?.consent_enabled && <span className={`badge ${upnp.state === 'MAPPED' ? 'online' : upnp.state === 'FAILED' ? 'warn' : ''}`}>{upnp.state === 'MAPPED' ? t('Router set up ✓', 'Modem ayarlandı ✓') : upnp.state === 'FAILED' ? t("Router didn't allow it", 'Modem izin vermedi') : t('Trying…', 'Deneniyor…')}</span>}</div>
      <p className="muted">{t('Needed when your collaborator is on a different internet connection. Opens one port for TrackMesh only; it closes when you quit TrackMesh.', "İş arkadaşın farklı bir internet bağlantısındaysa gerekir. Yalnızca TrackMesh için bir port açar; TrackMesh'ten çıkınca kapanır.")}
        {upnp?.consent_enabled && upnp.state === 'FAILED' ? t(' See Advanced → Connection.', ' Gelişmiş → Bağlantı bölümüne bak.') : ''}</p>
    </section>

    <section className="settings-block"><h3>{t('Where new projects go', 'Yeni projelerin yeri')}</h3>
      <p className="muted">{t('Projects you get from an invite link are saved in this folder.', 'Davet bağlantısıyla aldığın projeler bu klasöre kaydedilir.')}</p>
      <div className="input-row"><button className="path-link" title={t('Open in Explorer', "Gezgin'de aç")} onClick={() => void openProjectsLocation().catch(err => onError(String(err)))}>{displayPath(projectsLocation)}</button>
        <button disabled={busy} onClick={async () => { try { const value = await chooseProjectsLocation(); if (value) { setProjectsLocation(value); await poll(); } } catch (err) { onError(String(err)); } }}>{t('Change…', 'Değiştir…')}</button></div>
    </section>

    <section className="settings-block"><h3>{t('Language', 'Dil')}</h3>
      <div className="segmented" role="radiogroup" aria-label={t('Language', 'Dil')}>{LANGUAGES.map(item => <button key={item.value} role="radio" lang={item.value} aria-checked={lang === item.value} className={lang === item.value ? 'on' : ''} onClick={() => setLang(item.value)}>{item.label}</button>)}</div>
    </section>

    <section className="settings-block"><h3>{t('Text size', 'Yazı boyutu')}</h3>
      <div className="segmented" role="radiogroup" aria-label={t('Text size', 'Yazı boyutu')}>{ZOOM_LEVELS.map(level => <button key={level.label} role="radio" aria-checked={zoom === level.value} className={zoom === level.value ? 'on' : ''} onClick={() => setZoom(level.value)}>{zoomLabel(level.label)}</button>)}</div>
      <p className="muted">{t('Also Ctrl + plus, Ctrl + minus and Ctrl + 0.', 'Ctrl + artı, Ctrl + eksi ve Ctrl + 0 ile de olur.')}</p>
    </section>

    <StorageSettings busy={busy} act={act} />
    <p className="muted">{t('Connection details, invite service and manual setups are in Advanced.', "Bağlantı ayrıntıları, davet servisi ve elle kurulumlar Gelişmiş'te.")}</p>
  </section>;
}
