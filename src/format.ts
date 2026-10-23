import type { Catalog } from './api';

export const plural = (n: number, word: string) => (n === 1 ? `1 ${word}` : `${n} ${word}s`);

/** `now` is passed in (unix seconds) so callers render the same text for the same state. */
export function ago(unix: number, now: number) {
  const s = now - unix;
  if (s < 60) return 'just now';
  if (s < 3600) return `${plural(Math.floor(s / 60), 'minute')} ago`;
  if (s < 86400) return `${plural(Math.floor(s / 3600), 'hour')} ago`;
  return `${plural(Math.floor(s / 86400), 'day')} ago`;
}

/** Groups paths under home by their plain folder name, keeping the file names. */
export function byFolder(paths: string[], catalog: Catalog): [string, string[]][] {
  const name = (path: string) => {
    if (path === catalog.data_label) return path;
    // Only the background in use syncs, kept apart from the folders (see save_background).
    if (path.startsWith('.background/')) return 'Background';
    const match = catalog.folders
      .filter((f) => path === f.path || path.startsWith(`${f.path}/`))
      .toSorted((a, b) => b.path.length - a.path.length)[0];
    return match ? match.name : path;
  };
  const groups = new Map<string, string[]>();
  for (const path of paths) {
    const key = name(path);
    if (!groups.has(key)) groups.set(key, []);
    if (path !== catalog.data_label) groups.get(key)!.push(path.split('/').pop()!);
  }
  return [...groups.entries()].toSorted((a, b) => a[0].localeCompare(b[0]));
}

export function shortList(files: string[], max = 4) {
  const shown = files.slice(0, max).join(', ');
  return files.length > max ? `${shown} and ${files.length - max} more` : shown;
}

/**
 * Where a job stands: the steps reached (the last in progress) then the announced ones still to
 * come, and how much of it is done, counting the current step's percentage.
 */
export function progress(steps: string[], plan: string[], percent: number | null) {
  const items = [...steps, ...plan.filter((p) => !steps.includes(p))];
  const done = Math.max(steps.length - 1, 0) + (percent ?? 0) / 100;
  return { items, share: items.length ? Math.min(done / items.length, 1) : 0 };
}
