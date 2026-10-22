import { useCallback, useEffect, useState } from 'react';
import { getStorage, openPreviousVersion } from './api';
import type { Action, StorageSummary } from './api';
import { bytes as size, time } from './format';
import { t } from './i18n';

export function StorageSettings({ busy, act }: { busy: boolean; act: (action: Action) => Promise<boolean> }) {
  const [storage, setStorage] = useState<StorageSummary | null>(null);
  const [selected, setSelected] = useState<string[]>([]);
  const [error, setError] = useState('');
  const [loading, setLoading] = useState(false);
  const refresh = useCallback(async () => {
    setLoading(true);
    try { setStorage(await getStorage()); setSelected([]); setError(''); }
    catch (error) { setError(String(error)); }
    finally { setLoading(false); }
  }, []);
  useEffect(() => { if (!busy) void refresh(); }, [busy, refresh]);
  const chosen = storage?.backups.filter(backup => !backup.protected && selected.includes(backup.transfer_id)) ?? [];
  const total = chosen.reduce((sum, backup) => sum + backup.bytes, 0);
  return <section className="settings-block"><h3>{t('Storage', 'Depolama')}</h3>
    <p>{t('TrackMesh keeps what it needs by itself: your latest version, the latest version from each collaborator and any Get that has not finished.',
      'TrackMesh ihtiyaç duyduğunu kendisi tutar: senin son sürümün, her iş arkadaşından gelen son sürüm ve bitmemiş almalar.')}</p>
    {storage?.cache_bytes != null && <p>{t('App file data:', 'Uygulama dosya verisi:')} <strong>{size(storage.cache_bytes)}</strong></p>}
    <p>{t('When you get a version, your previous files are kept next to the project as a backup. They are deleted only here, and the newest backup of each project always stays.',
      'Bir sürüm aldığında önceki dosyaların projenin yanında yedek olarak kalır. Yalnızca buradan silinir; her projenin en yeni yedeği hep kalır.')}</p>
    {loading && <p className="muted">{t('Reading backup sizes…', 'Yedek boyutları okunuyor…')}</p>}
    {error && <p className="error">{error}</p>}
    {storage?.problems.map(problem => <p className="error" key={problem}>{problem}</p>)}
    {storage && !storage.backups.length && <p className="muted">{t('No completed backups available.', 'Tamamlanmış yedek yok.')}</p>}
    {!!storage?.backups.length && <><p><label><input type="checkbox" disabled={busy || loading} checked={storage.backups.some(b => !b.protected) && storage.backups.filter(b => !b.protected).every(b => selected.includes(b.transfer_id))}
      onChange={event => setSelected(event.target.checked ? storage.backups.filter(b => !b.protected).map(b => b.transfer_id) : [])} /> {t('Select all old backups', 'Tüm eski yedekleri seç')}</label></p>
      <ul>{storage.backups.map(backup => <li key={backup.transfer_id}>
        <label><input type="checkbox" disabled={busy || loading || backup.protected} checked={selected.includes(backup.transfer_id)} onChange={event => setSelected(ids => event.target.checked ? [...ids, backup.transfer_id] : ids.filter(id => id !== backup.transfer_id))} />
          {backup.project_name} · {size(backup.bytes)}{backup.modified_at != null && ` · ${time(new Date(backup.modified_at * 1000).toISOString())}`} · {backup.protected ? t('Kept', 'Tutuluyor') : t('Old backup', 'Eski yedek')}</label>
        <button disabled={busy || loading} onClick={() => void openPreviousVersion(backup.project_id, backup.transfer_id).catch(err => setError(String(err)))}>{t('Open', 'Aç')}</button>
        <p className="muted"><code>{backup.path.replace(/^\\\\\?\\/, '')}</code></p>
      </li>)}</ul>
      <button className="danger" disabled={busy || loading || !chosen.length || chosen.length > 1024} onClick={() => void act({ kind: 'delete_backups', selected: chosen.map(({ transfer_id, path, bytes }) => ({ transfer_id, path, bytes })) })}>{t('Delete selected backups', 'Seçili yedekleri sil')} · {size(total)}</button>
    </>}
    <p className="muted">{t('Get asks first when a version needs more than 1 GB of new data. Parts already on this PC are not downloaded again.', "Bir sürüm 1 GB'tan fazla yeni veri gerektiriyorsa Al önce sorar. Bu bilgisayarda zaten olan kısımlar yeniden inmez.")}</p>
  </section>;
}
