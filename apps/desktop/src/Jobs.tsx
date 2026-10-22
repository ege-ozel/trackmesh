import { useEffect, useState } from 'react';
import type { ReactNode } from 'react';
import type { DesktopStatus } from './api';
import { CHECK_TIMEOUT_SECONDS, bytes, displayPath } from './format';
import { lang, named, t } from './i18n';

type Job = DesktopStatus['jobs'][number];

/** Closing a file picker is not a failure; such results show nothing. */
// Things the person cancelled themselves; the card simply goes back to how it was.
export const cancelled = (raw?: string | null) => !!raw && /File selection cancelled|Folder change cancelled|Download cancelled/i.test(raw);

/** Turkish for backend errors a person can reach, keyed by the start of the English text; the raw text stays under Details. */
const TURKISH_ERRORS: [string, string][] = [
  ['Choose a .json filename', '.json uzantılı bir dosya adı seç.'],
  ['Choose a folder for the project', 'Proje için bir klasör seç.'],
  ['Choose a new filename', 'Yeni bir dosya adı seç; var olan dosyalar korunur.'],
  ['Could not open folder in Explorer', "Klasör Gezgin'de açılamadı."],
  ['Desktop is starting', 'TrackMesh açılıyor ya da kapanıyor. Biraz bekle, sonra tekrar dene.'],
  ['Desktop session is closing', 'TrackMesh kapanıyor.'],
  ['Engine is unavailable', 'TrackMesh henüz hazır değil. Biraz bekle, sonra tekrar dene.'],
  ['Enter both the Supabase Project URL and publishable key', 'Supabase proje adresini ve yayımlanabilir anahtarı gir.'],
  ['Enter the Supabase Project URL', 'Supabase proje adresini gir.'],
  ['Enter the Supabase publishable key', 'Supabase yayımlanabilir anahtarını gir.'],
  ['Folder is not available', 'Klasöre ulaşılamıyor.'],
  ['Invalid TrackMesh JSON document', 'Bu dosya bir TrackMesh dosyası değil ya da bozuk.'],
  ['Invitation accepted, but network reload failed', "Davet kabul edildi ama ağ yeniden başlatılamadı. TrackMesh'i kapatıp aç."],
  ['Invite accepted, but network reload failed', "Davet kabul edildi ama ağ yeniden başlatılamadı. TrackMesh'i kapatıp aç."],
  ['Project setup saved, but network reload failed', "Proje kaydedildi ama ağ yeniden başlatılamadı. TrackMesh'i kapatıp aç."],
  ['Only the device that created this project can invite or approve collaborators', 'Yalnızca projeyi oluşturan cihaz davet edebilir ve onaylayabilir.'],
  ['Invite link could not be created', 'Davet bağlantısı oluşturulamadı. İnternet bağlantını kontrol et, sonra tekrar dene.'],
  ['Invite link is invalid, expired, or unavailable', 'Davet bağlantısı geçersiz, süresi dolmuş ya da kullanılmış. Yeni bir bağlantı iste.'],
  ['invalid invitation link', 'Davet bağlantısı geçersiz. Bağlantıyı yeniden kopyala.'],
  ['Invite links require', 'Davet bağlantıları için Supabase adresi https://<ref>.supabase.co biçiminde olmalı.'],
  ['No available project folder name', 'Bu adla boş bir proje klasörü oluşturulamadı. Başka bir yer seç.'],
  ['No invitation link is waiting', 'Bekleyen bir davet bağlantısı yok.'],
  ['No invite link is open', 'Açık bir davet bağlantısı yok.'],
  ['No pairing window for this project', 'Bu proje için açık bir onay penceresi yok.'],
  ['Nothing is running for this project', 'Bu projede süren bir iş yok.'],
  ['Pairing document exceeds 64 KiB', 'Eşleşme dosyası çok büyük.'],
  ['Previous version backup is not available', 'Önceki dosyaların yedeği bulunamadı.'],
  ['Project folder name is empty', 'Proje klasörünün adı boş olamaz.'],
  ['Public IPv4 and external port must be supplied together', 'Genel IPv4 adresini ve dış portu birlikte gir.'],
  ['Select and save Supabase for this project', 'Davet bağlantısı oluşturmadan önce bu proje için davet servisini seçip kaydet.'],
  ['Selected folder could not be resolved', 'Seçilen klasöre ulaşılamadı.'],
  ['Supabase is not configured', 'Davet servisi ayarlanmamış.'],
  ['The selected Projects location could not be', 'Seçilen proje konumu oluşturulamadı ya da ulaşılamadı.'],
  ["This operation can't be stopped", 'Bu iş durdurulamaz.'],
  ['Too many active operations', 'Çok fazla iş sürüyor. Biri bitince tekrar dene.'],
  ['Windows Documents folder is unavailable', 'Windows Belgeler klasörüne ulaşılamıyor.'],
  ['Windows clipboard', 'Windows panosu kullanılamadı. Tekrar dene.'],
  ['This folder was removed from TrackMesh', "Bu klasör TrackMesh'ten kaldırılmıştı. Ayarlar'daki Kaldırılan projeler bölümünden geri getir."],
  ['project is not on the list', 'Bu proje listede değil.'],
  ['project cannot be restored', 'Bu proje geri getirilemiyor.'],
];

/** A red or amber box stays until its × is pressed; a new run of the same action is a new job and replaces it. */
const dismissed = new Set<string>();
export function CloseButton({ onClose }: { onClose: () => void }) {
  return <button className="close-x" aria-label={t('Close', 'Kapat')} title={t('Close', 'Kapat')} onClick={onClose}>×</button>;
}

/** Backend job labels are stable identifiers; these are the words the buttons use. */
export function jobLabel(label: string, collaboratorName?: string) {
  const who = collaboratorName ?? 'your collaborator';
  switch (label) {
    case 'Delete old backups': return t('Delete old backups', 'Eski yedekleri sil');
    case 'Link folder': return t('Add project', 'Proje ekle');
    case 'Change project folder': return t('Change project folder', 'Proje klasörünü değiştir');
    case 'Save project backend': return t('Save invite service', 'Davet servisini kaydet');
    case 'Save default Supabase settings': return t('Save default invite service', 'Varsayılan davet servisini kaydet');
    case 'Save device name': return t('Save your name', 'Adını kaydet');
    case 'Save DAW auto-share setting': return t('Save share-on-save', 'Kaydedince paylaş ayarı');
    case 'Save Projects location': return t('Save projects location', 'Proje konumunu kaydet');
    case 'Save manual port forwarding': return t('Save manual port forwarding', 'Elle port yönlendirmeyi kaydet');
    case 'Accept invitation and request pairing': return t('Open invitation file', 'Davet dosyasını aç');
    case 'Accept invite link': return t('Accept invite link', 'Davet bağlantısını kabul et');
    case 'Complete pairing': return t('Open pairing reply', 'Eşleşme yanıtını aç');
    case 'Revoke device': return t('Remove access', 'Erişimi kaldır');
    case 'Remove project': return t('Remove from TrackMesh', "TrackMesh'ten kaldır");
    case 'Restore project': return t('Bring back', 'Geri getir');
    case 'Export invitation': return t('Save invitation file', 'Davet dosyasını kaydet');
    case 'Copy invite link': return t('Copy invite link', 'Davet bağlantısını kopyala');
    case 'Save pairing reply': return t('Save pairing reply', 'Eşleşme yanıtını kaydet');
    case 'Open pairing window': return t('Open approval window', 'Onay penceresini aç');
    case 'Close pairing window': return t('Close approval window', 'Onay penceresini kapat');
    case 'Approve pairing': return t('Approve', 'Onayla');
    case 'Reject pairing': return t('Reject', 'Reddet');
    case 'Request automatic pairing': return t('Ask for approval', 'Onay iste');
    case 'Add known device': return t('Add device', 'Cihazı ekle');
    case 'Accept project offer': return t('Accept project', 'Projeyi kabul et');
    case 'Decline project offer': return t('Decline project', 'Projeyi reddet');
    case 'Open invite link': return t('Invite link', 'Davet bağlantısı');
    case 'Share current version': return t('Share current version', 'Güncel sürümü paylaş');
    case 'Check connection': return t('Check connection', 'Bağlantıyı kontrol et');
    case "Get collaborator's version": return t(`Get ${who}'s version`, `${named(collaboratorName, 'gen')} sürümünü al`);
    case "Use collaborator's version": return t(`Use ${who}'s version`, `${named(collaboratorName, 'gen')} sürümünü kullan`);
    case 'Recover installation': return t('Recover', 'Kurtar');
    default: return label;
  }
}

/** Turkish for the result messages the backend writes in English; unknown text stays as it is. */
const TURKISH_MESSAGES: Record<string, string> = {
  'Removed from TrackMesh. The folder and its files are unchanged.': "TrackMesh'ten kaldırıldı. Klasör ve dosyaları değişmedi.",
  'Back in TrackMesh. Invite your collaborator again to share it.': "Proje TrackMesh'e geri geldi. Paylaşmak için iş arkadaşını yeniden davet et.",
  "This folder has changes you haven't shared. Choose which version to keep.": 'Bu klasörde paylaşmadığın değişiklikler var. Hangi sürümün kalacağını seç.',
  'Both versions changed. Choose which version to keep.': 'İki sürüm de değişti. Hangi sürümün kalacağını seç.',
  'Connection check stopped': 'Bağlantı kontrolü durduruldu',
  'Sharing stopped. Nothing was shared.': 'Paylaşım durduruldu. Hiçbir şey paylaşılmadı.',
  'Getting stopped. Anything already downloaded is kept for Resume getting.': 'Alma durduruldu. İnen kısım, Almaya devam et için saklandı.',
  'Sharing cancelled': 'Paylaşım iptal edildi',
  'Collaborator approved. Add your project files, then Share.': 'İş arkadaşın onaylandı. Proje dosyalarını ekle, sonra paylaş.',
  'Collaborator approved. First version shared and ready to receive.': 'İş arkadaşın onaylandı. İlk sürüm paylaşıldı, alınmaya hazır.',
  'Collaborator approved for this project.': 'İş arkadaşın bu proje için onaylandı.',
  'Local recovery completed': 'Kurtarma tamamlandı',
  'Offer sent. The other PC accepts it in TrackMesh.': 'Teklif gönderildi. Diğer bilgisayar TrackMesh\'te kabul edince proje eklenir.',
  'Offer declined': 'Teklif reddedildi',
  'Folder linked': 'Proje eklendi',
  'Project folder updated': 'Proje klasörü güncellendi',
  'Project backend settings saved': 'Davet servisi ayarları kaydedildi',
  'Default Supabase settings saved': 'Varsayılan davet servisi kaydedildi',
  'Device name saved': 'Adın kaydedildi',
  'Automatic sharing is on': 'Kaydedince paylaş açık',
  'Automatic sharing is off': 'Kaydedince paylaş kapalı',
  'Projects location saved': 'Proje konumu kaydedildi',
  'Manual port forwarding settings saved': 'Elle port yönlendirme ayarları kaydedildi',
  'Approval window open for 15 minutes. Ask your collaborator to press Ask for approval, then verify the arriving device before approving.': 'Onay penceresi 15 dakika açık. İş arkadaşından Onay iste\'ye basmasını iste; onaylamadan önce gelen cihazı kontrol et.',
  'Pairing window closed; unapproved requests removed': 'Onay penceresi kapandı; onaylanmamış istekler silindi',
  'Pairing request rejected; no project access was granted': 'İstek reddedildi; projeye erişim verilmedi',
  'Pairing reply accepted. This device is authorized for the selected project. Tell your collaborator pairing is complete, create a snapshot, and keep TrackMesh open.': 'Eşleşme yanıtı kabul edildi. Bu cihaz projeye yetkili. İş arkadaşına eşleşmenin bittiğini söyle, bir sürüm paylaş ve TrackMesh\'i açık tut.',
  'Pairing reply saved. Send this file privately to the inviter to complete pairing.': 'Eşleşme yanıtı kaydedildi. Eşleşmeyi bitirmek için bu dosyayı davet edene özel olarak gönder.',
  'Private invitation saved. Automatic pairing window open for 15 minutes; share the invitation privately, then verify and approve your collaborator here.': 'Özel davet dosyası kaydedildi. Onay penceresi 15 dakika açık; daveti özel olarak gönder, sonra iş arkadaşını burada kontrol edip onayla.',
  'Pairing request sent automatically. The inviter must verify and approve your device. No card file needs to be sent; wait for their confirmation.': 'İstek gönderildi. Davet eden kişi cihazını kontrol edip onaylamalı; dosya göndermene gerek yok, onayı bekle.',
  'The inviter approved this device for the project.': 'Davet eden kişi bu cihazı projeye onayladı.',
  'Automatic pairing could not complete. Your project remains imported. Ask the inviter to open the approval window and keep TrackMesh open, then press Ask for approval. Do not reimport or create a new UUID. Both devices need a build supporting automatic pairing.': 'Onay isteği tamamlanamadı. Proje bu bilgisayarda duruyor. Davet edenden onay penceresini açmasını ve TrackMesh\'i açık tutmasını iste, sonra Onay iste\'ye bas. Daveti yeniden açma.',
};

export function backendMessage(message: string) {
  if (lang() !== 'tr') return message;
  const known = TURKISH_MESSAGES[message];
  if (known) return known;
  let match = message.match(/^Deleted (\d+) bytes of old backups$/);
  if (match) return `${bytes(Number(match[1]))} eski yedek silindi`;
  match = message.match(/^(Private invitation saved\. )?Pending approvals for (.+) were cleared; only one project can accept new devices at a time\.$/);
  if (match) return `${match[1] ? 'Özel davet dosyası kaydedildi. ' : ''}${match[2]} için bekleyen onaylar silindi; aynı anda yalnızca bir proje yeni cihaz kabul edebilir.`;
  match = message.match(/^Removed (.+)'s access on this PC\.(.*)$/);
  if (match) {
    const who = match[1];
    const rest = match[2].includes('will also remove') ? ` TrackMesh, ${named(who, 'acc')} davet servisinden de çıkaracak.`
      : match[2].includes('Only the device') ? ` ${named(who, 'acc')} davet servisinden yalnızca projeyi oluşturan cihaz çıkarabilir.` : '';
    return `${named(who, 'gen')} bu bilgisayardaki erişimi kaldırıldı.${rest}`;
  }
  return message;
}

/** Plain-language reading of the backend errors people actually hit; null keeps the raw text. */
export function friendlyError(raw: string, collaboratorName?: string): string | null {
  const who = collaboratorName ?? 'your collaborator';
  if (/offline or unreachable|request timed out|dial|connection (refused|reset|closed)|network stopped|no addresses|not connected/i.test(raw))
    return t(`Couldn't reach ${who}. Make sure TrackMesh is open on both PCs, then try again.`, `${named(collaboratorName, 'dat')} ulaşılamadı. TrackMesh'in iki bilgisayarda da açık olduğundan emin ol, sonra tekrar dene.`);
  if (/Close the DAW project before/i.test(raw))
    return t('Close this project in your DAW, then try again.', "Projeyi DAW'da kapat, sonra tekrar dene.");
  if (/still being written or locked|changed (before|during) scan|directory changed during scan|did not settle|changed while saving/i.test(raw))
    return t('Your DAW is still saving or has a file open. Wait a moment, then try again.', "DAW hâlâ kaydediyor ya da bir dosya açık. Biraz bekle, sonra tekrar dene.");
  if (/recovery must finish|recover the existing interrupted/i.test(raw))
    return t('The last Get did not finish. Press Recover on this card first.', 'Son alma işlemi bitmedi. Önce bu karttaki Kurtar\'a bas.');
  if (/original changed or is locked/i.test(raw))
    return t('A project file was open or changed while TrackMesh put the new files in place. Close the project in your DAW, then press Recover.',
      "TrackMesh yeni dosyaları yerleştirirken bir proje dosyası açıktı ya da değişti. Projeyi DAW'da kapat, sonra Kurtar'a bas.");
  if (/Access is denied|os error 5\b/i.test(raw))
    return t('Windows did not allow a project file or folder to change. Close the project in your DAW and any Explorer window showing it, then try again.',
      "Windows bir proje dosyasının ya da klasörünün değişmesine izin vermedi. Projeyi DAW'da ve onu gösteren Gezgin pencerelerinde kapat, sonra tekrar dene.");
  if (/already (has an )?active|Snapshot work is already active|Wait for the current operation/i.test(raw))
    return t('TrackMesh is already working on this project. Wait for it to finish.', 'TrackMesh bu projede zaten çalışıyor. Bitmesini bekle.');
  if (/author identity is not trusted/i.test(raw))
    return t(`${who}'s version was saved by a device this PC doesn't know. Both of you should update TrackMesh, then ${who} shares again.`,
      `${named(collaboratorName, 'gen')} sürümünü bu bilgisayarın tanımadığı bir cihaz kaydetmiş. İkiniz de TrackMesh'i güncelleyin, sonra ${who} yeniden paylaşsın.`);
  if (/not paired|pairing rejected|not authorized/i.test(raw))
    return t(`This device isn't approved for this project on ${who}'s PC.`, `Bu cihaz ${named(collaboratorName, 'gen')} bilgisayarında bu proje için onaylı değil.`);
  if (/local head advanced/i.test(raw))
    return t('You shared a newer version while this Get was paused. Press Get again.', 'Alma duraklatılmışken daha yeni bir sürüm paylaştın. Tekrar Al\'a bas.');
  if (/An invite is being accepted/i.test(raw))
    return t('An invite is being accepted. Wait for it to finish before opening another link.', 'Bir davet kabul ediliyor. Başka bir bağlantı açmadan önce bitmesini bekle.');
  if (/This invite link could not be opened/i.test(raw))
    return t('This invite link could not be opened. Check the link and try again.', 'Bu davet bağlantısı açılamadı. Bağlantıyı kontrol edip tekrar dene.');
  if (/The other PC could not be reached/i.test(raw))
    return t(`Couldn't reach ${who}. Make sure TrackMesh is open on both PCs, then try again.`, `${named(collaboratorName, 'dat')} ulaşılamadı. TrackMesh'in iki bilgisayarda da açık olduğundan emin ol, sonra tekrar dene.`);
  if (/The other PC did not take the offer/i.test(raw))
    return t('The other PC did not take the offer. Both PCs need the latest TrackMesh, and the project must not already be on that PC.', 'Diğer bilgisayar teklifi almadı. İki bilgisayarda da güncel TrackMesh olmalı ve proje o bilgisayarda zaten olmamalı.');
  if (/the offer expired/i.test(raw))
    return t('This offer expired. Ask for it again.', 'Bu teklifin süresi doldu. Yeniden göndermesini iste.');
  if (/No TrackMesh invite link on the clipboard/i.test(raw))
    return t('No invite link on the clipboard. Copy the link again, then press Paste invite link.', "Panoda davet bağlantısı yok. Bağlantıyı yeniden kopyala, sonra Davet bağlantısını yapıştır'a bas.");
  if (lang() === 'tr') {
    const known = TURKISH_ERRORS.find(([english]) => raw.startsWith(english));
    if (known) return known[1];
  }
  if (/Backup deletion cancelled/i.test(raw))
    return t('Nothing was deleted.', 'Hiçbir şey silinmedi.');
  if (/Download cancelled/i.test(raw))
    return t('Download cancelled. What was already downloaded is kept for Resume getting.', 'İndirme iptal edildi. İnen kısım, Almaya devam et için saklandı.');
  if (/exceeds MVP size limit|too many project files/i.test(raw))
    return t('This project is larger than TrackMesh supports yet.', 'Bu proje TrackMesh\'in şimdilik desteklediğinden büyük.');
  if (/BLAKE3|unexpected .*response|ancestry|signature/i.test(raw))
    return t(`${who}'s PC sent data that didn't verify. Try again; if it keeps failing, both of you should update TrackMesh.`,
      `${named(collaboratorName, 'gen')} bilgisayarından gelen veri doğrulanamadı. Tekrar dene; sürerse ikiniz de TrackMesh'i güncelleyin.`);
  return null;
}

/** `online` is the collaborator's live state, so a Share result stays true as the connection changes. */
export function jobStatus(job: Job, startedAt: number | undefined, collaboratorName?: string, online?: boolean) {
  const who = collaboratorName ?? 'your collaborator';
  const label = jobLabel(job.label, collaboratorName);
  if (!job.running) {
    if (job.error) return friendlyError(job.error, collaboratorName) ?? `${label}: ${job.error}`;
    if (job.label === "Use collaborator's version" && typeof job.outcome?.transfer_id === 'string') return t(`Now using ${who}'s version ✓`, `Artık ${named(collaboratorName, 'gen')} sürümü kullanılıyor ✓`);
    if (job.label === "Get collaborator's version" && typeof job.outcome?.transfer_id === 'string') return t(`Got ${who}'s version ✓`, `${named(collaboratorName, 'gen')} sürümü alındı ✓`);
    if (job.label === 'Share current version' && typeof job.outcome?.snapshot_id === 'string') {
      if (!collaboratorName) return t('Saved as a new version.', 'Yeni sürüm olarak kaydedildi.');
      return (online ? t(`Shared — ${who} can get this version now.`, `Paylaşıldı — ${who} bu sürümü şimdi alabilir.`)
        : t(`Saved as a new version. ${who} can get it when you are both online.`, `Yeni sürüm olarak kaydedildi. İkiniz de bağlı olunca ${who} alabilir.`));
    }
    return job.outcome?.message ? backendMessage(String(job.outcome.message)) : `${label} ✓`;
  }
  if (job.label === 'Share current version') return t('Preparing current version…', 'Güncel sürüm hazırlanıyor…');
  if (job.label !== 'Check connection') return `${label}…`;
  const elapsed = startedAt ? Math.floor((Date.now() - startedAt) / 1000) : 0;
  const remaining = Math.max(0, CHECK_TIMEOUT_SECONDS - elapsed);
  return t(`Checking the connection to ${who}… up to ${remaining}s left`, `${who} ile bağlantı kontrol ediliyor… en fazla ${remaining} sn kaldı`);
}

/** Work this PC is doing right now; an indeterminate line under the text, never a spinning frame. */
export function InProgress({ children, action }: { children: ReactNode; action?: ReactNode }) {
  return <div className="in-progress" role="status"><span>{children}</span>{action}</div>;
}

/** Waiting on another person: a static line, since nothing here is working. */
export function Waiting({ children, action }: { children: ReactNode; action?: ReactNode }) {
  return <div className="waiting-line" role="status"><span>{children}</span>{action}</div>;
}

/** The raw backend text stays one click away whenever a friendly line replaced it. */
export function ErrorDetails({ raw, collaboratorName }: { raw: string; collaboratorName?: string }) {
  if (!friendlyError(raw, collaboratorName)) return null;
  return <details className="error-details"><summary>{t('Details', 'Ayrıntılar')}</summary><code>{raw}</code></details>;
}

const STOPPABLE = new Set(['Check connection', 'Share current version', "Get collaborator's version", "Use collaborator's version"]);

/** Pass `onStop` only while stopping is safe; the backend never interrupts installing files. */
export function JobNotice({ job, startedAt, collaboratorName, online, onStop, children }: {
  job: Job; startedAt: number | undefined; collaboratorName?: string; online?: boolean; onStop?: () => void; children?: ReactNode;
}) {
  const [, setClosed] = useState(0);
  const text = jobStatus(job, startedAt, collaboratorName, online);
  if (!job.running && (cancelled(job.error) || dismissed.has(job.job_id))) return null;
  if (job.running) return <InProgress action={onStop && STOPPABLE.has(job.label) ? <StopButton onStop={onStop} /> : undefined}>{text}</InProgress>;
  const sharedOffline = job.label === 'Share current version' && typeof job.outcome?.snapshot_id === 'string' && !!collaboratorName && !online;
  const tone = job.error || job.outcome?.pairing_state === 'retry' ? 'danger' : sharedOffline ? 'warn' : '';
  // A finished job is a green result line; boxes are only for warnings and errors.
  return <div className={tone ? `job ${tone} closable` : 'delivery-receipt'} role="status">
    {tone && <CloseButton onClose={() => { dismissed.add(job.job_id); setClosed(n => n + 1); }} />}
    {text}{children}
    {job.error && <ErrorDetails raw={job.error} collaboratorName={collaboratorName} />}
    {typeof job.outcome?.path === 'string' && <p>{t('Saved file:', 'Kaydedilen dosya:')} <code>{displayPath(job.outcome.path)}</code></p>}
  </div>;
}

/** Stays pressed after a click: the job ends at its next safe point, which can take a moment. */
export function StopButton({ onStop }: { onStop: () => void }) {
  const [stopping, setStopping] = useState(false);
  return <button disabled={stopping} onClick={() => { setStopping(true); onStop(); }}>{stopping ? t('Stopping…', 'Durduruluyor…') : t('Stop', 'Durdur')}</button>;
}

export function InviteCopiedNotice({ job }: { job: Job }) {
  const [now, setNow] = useState(Date.now());
  useEffect(() => {
    const interval = window.setInterval(() => setNow(Date.now()), 1000);
    return () => window.clearInterval(interval);
  }, []);
  const remaining = Math.max(0, 60 - Math.floor((now - (job.completed_at ?? Math.floor(now / 1000)) * 1000) / 1000));
  if (!remaining) return <div className="job" role="status">{t('Clipboard cleared. Copy the invite link again if it was not pasted.', 'Pano temizlendi. Bağlantı yapıştırılmadıysa yeniden kopyala.')}</div>;
  // The link lives 15 minutes and opens once; only the clipboard copy is cleared after 60 s.
  return <div className="job" role="status">{t('Link copied. Valid for 15 minutes and works once. For safety it is removed from the clipboard after 60 s.',
    'Bağlantı kopyalandı. 15 dakika ve bir kez geçerli. Güvenlik için 60 sn sonra panodan silinir.')}</div>;
}
