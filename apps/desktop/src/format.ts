import type { DesktopStatus } from './api';
import { t } from './i18n';

export const short = (id: string | null | undefined) => id ? id.slice(0, 8) : '—';
const pad = (n: number) => String(n).padStart(2, '0');
/** One date format in the app and the plugin: 14.10.2026 16:05. */
export const time = (value: string | null | undefined) => {
  if (!value) return t('Never', 'Hiç');
  const d = new Date(value);
  return `${pad(d.getDate())}.${pad(d.getMonth() + 1)}.${d.getFullYear()} ${pad(d.getHours())}:${pad(d.getMinutes())}`;
};
export const bytes = (n: number) => n >= 1024 ** 3 ? `${(n / 1024 ** 3).toFixed(1)} GB` : n >= 1024 ** 2 ? `${(n / 1024 ** 2).toFixed(1)} MB` : n >= 1024 ? `${(n / 1024).toFixed(1)} KB` : `${n} B`;
export const CHECK_TIMEOUT_SECONDS = 30;
export const displayPath = (path: string) => path.replace(/^\\\\\?\\/, '');

export function localIpv4Addresses(status: DesktopStatus) {
  return [...new Set((status.network?.addresses ?? []).flatMap(address => {
    const match = address.match(/^\/ip4\/([^/]+)\/(tcp|udp)\/(\d+)(?:\/|$)/);
    return match && match[1] !== '0.0.0.0' && !match[1].startsWith('127.') && Number(match[3]) === status.listen_port ? [match[1]] : [];
  }))];
}

export function publicIpv4(value: string) {
  const parts = value.split('.');
  if (parts.length !== 4 || parts.some(p => !/^(0|[1-9]\d{0,2})$/.test(p) || Number(p) > 255)) return false;
  const [a, b, c] = parts.map(Number);
  return a > 0 && a < 224 && a !== 10 && a !== 127
    && !(a === 100 && b >= 64 && b <= 127) && !(a === 169 && b === 254)
    && !(a === 172 && b >= 16 && b <= 31) && !(a === 192 && b === 168)
    && !(a === 192 && b === 0 && c === 2) && !(a === 198 && b === 51 && c === 100)
    && !(a === 203 && b === 0 && c === 113) && !(a === 198 && (b === 18 || b === 19));
}
