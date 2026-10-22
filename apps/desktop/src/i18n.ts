/** Interface language. Strings stay next to their use as t('English', 'Türkçe'). */
export type Lang = 'en' | 'tr';

export const LANGUAGES: { value: Lang; label: string }[] = [{ value: 'en', label: 'English' }, { value: 'tr', label: 'Türkçe' }];

let current: Lang = 'en';

export function readLang(): Lang {
  try {
    const saved = window.localStorage.getItem('trackmesh.lang');
    if (saved === 'en' || saved === 'tr') return saved;
  } catch { /* fall back to the system language */ }
  return navigator.language?.toLowerCase().startsWith('tr') ? 'tr' : 'en';
}

export function saveLang(lang: Lang) {
  try { window.localStorage.setItem('trackmesh.lang', lang); } catch { /* per-viewer convenience only */ }
}

/** App sets this during render, before any child reads it. */
export function setLang(lang: Lang) { current = lang; }
export const lang = () => current;

export const t = (en: string, tr: string) => current === 'tr' ? tr : en;

type Case = 'nom' | 'gen' | 'acc' | 'dat' | 'loc' | 'abl';

const UNNAMED: Record<Case, string> = {
  nom: 'iş arkadaşın', gen: 'iş arkadaşının', acc: 'iş arkadaşını', dat: 'iş arkadaşına', loc: 'iş arkadaşında', abl: 'iş arkadaşından',
};

/**
 * A collaborator's name with a Turkish case suffix chosen by vowel harmony, e.g. Ege'nin, Mert'e,
 * Burak'tan. An unknown name reads "iş arkadaşın…". English callers use the name as it is.
 */
// How a name ends when read aloud. A last word of one or two capitals or a digit is spelled out
// ("Studio B" → "be", "PC" → "ce", "Studio 2" → "iki"), so the suffix reads Studio B'nin, not Studio B'un.
const SPELLED: Record<string, string> = {
  A: 'a', B: 'be', C: 'ce', Ç: 'çe', D: 'de', E: 'e', F: 'fe', G: 'ge', Ğ: 'ge', H: 'he', I: 'ı', İ: 'i', J: 'je', K: 'ke', L: 'le',
  M: 'me', N: 'ne', O: 'o', Ö: 'ö', P: 'pe', Q: 'ku', R: 're', S: 'se', Ş: 'şe', T: 'te', U: 'u', Ü: 'ü', V: 've', W: 've', X: 'iks',
  Y: 'ye', Z: 'ze', 0: 'sıfır', 1: 'bir', 2: 'iki', 3: 'üç', 4: 'dört', 5: 'beş', 6: 'altı', 7: 'yedi', 8: 'sekiz', 9: 'dokuz',
};
function spoken(name: string): string {
  const word = name.trim().split(/\s+/).pop() ?? '';
  const end = name.trim().slice(-1);
  if ((/^[A-ZÇĞİÖŞÜ]{1,2}$/.test(word) || /[0-9]/.test(end)) && SPELLED[end]) return SPELLED[end];
  return name.toLocaleLowerCase('tr').replace(/â/g, 'a').replace(/î/g, 'i').replace(/û/g, 'u');
}

export function named(name: string | undefined, kind: Case = 'nom'): string {
  if (!name) return UNNAMED[kind];
  if (kind === 'nom') return name;
  const lower = spoken(name);
  const vowels = lower.match(/[aeıioöuü]/g);
  const last = vowels ? vowels[vowels.length - 1] : 'e';
  const end = lower.slice(-1);
  const endsVowel = /[aeıioöuü]/.test(end);
  const hard = /[çfhkpsşt]/.test(end);
  const two = /[eiöü]/.test(last) ? 'e' : 'a';
  const four = last === 'a' || last === 'ı' ? 'ı' : last === 'e' || last === 'i' ? 'i' : last === 'o' || last === 'u' ? 'u' : 'ü';
  const suffix = kind === 'gen' ? `${endsVowel ? 'n' : ''}${four}n`
    : kind === 'acc' ? `${endsVowel ? 'y' : ''}${four}`
    : kind === 'dat' ? `${endsVowel ? 'y' : ''}${two}`
    : kind === 'loc' ? `${hard ? 't' : 'd'}${two}`
    : `${hard ? 't' : 'd'}${two}n`;
  return `${name}'${suffix}`;
}
