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

/** One row of a side-by-side comparison: a line from each version, or a gap. */
export type DiffRow = { mine: string | null; theirs: string | null; changed: boolean };

/**
 * Two versions of a file side by side, line by line: lines both have line up, and lines only
 * one has (or that differ) are marked changed, paired across where they replace each other.
 */
export function diffLines(mine: string, theirs: string): DiffRow[] {
  const a = mine.split('\n');
  const b = theirs.split('\n');
  // Longest common run of lines, filled from the end; big files are compared line by line.
  const lcs =
    a.length * b.length > 4_000_000
      ? null
      : Array.from({ length: a.length + 1 }, () => Array.from({ length: b.length + 1 }, () => 0));
  if (lcs) {
    for (let i = a.length - 1; i >= 0; i--)
      for (let j = b.length - 1; j >= 0; j--)
        lcs[i][j] = a[i] === b[j] ? lcs[i + 1][j + 1] + 1 : Math.max(lcs[i + 1][j], lcs[i][j + 1]);
  }
  const rows: DiffRow[] = [];
  let [i, j] = [0, 0];
  let gone: string[] = [];
  let added: string[] = [];
  // Lines removed on one side and added on the other show on the same rows.
  const flush = () => {
    for (let k = 0; k < Math.max(gone.length, added.length); k++)
      rows.push({ mine: gone[k] ?? null, theirs: added[k] ?? null, changed: true });
    gone = [];
    added = [];
  };
  while (i < a.length || j < b.length) {
    if (i < a.length && j < b.length && a[i] === b[j]) {
      flush();
      rows.push({ mine: a[i], theirs: b[j], changed: false });
      i++;
      j++;
    } else if (j < b.length && (i >= a.length || !lcs || lcs[i][j + 1] >= lcs[i + 1][j])) {
      added.push(b[j++]);
    } else {
      gone.push(a[i++]);
    }
  }
  flush();
  return rows;
}

/** Said before a change to the system, where a snapshot of it is taken first. */
export const SNAPSHOT_FIRST =
  "A snapshot of this computer's system is taken first, so you can go back to it from the startup menu.";
