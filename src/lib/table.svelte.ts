// Client-side sorting and text filtering for the panel's tables.
//
// A page declares the columns it can sort by, keeps one `TableSort` in component state and renders
// `<SortHeader>` cells that toggle it. Clicking a header cycles that column through its natural
// direction, the reverse, and back to the table's own order, so a table with an opinionated default
// (the scoreboard by kills, the reserved list with online players first) is one click away again.

export type SortDir = 'asc' | 'desc';
export type SortValue = string | number | boolean | Date | null | undefined;

export interface SortColumn<T> {
	/** what to compare for a row; null, undefined and '' always sort last */
	by: (row: T) => SortValue;
	/** the direction the first click gives; newest and biggest first for dates and counts */
	dir?: SortDir;
}

/** what a `<SortHeader>` needs: the active column, its direction and a way to toggle */
export interface SortLike<K extends string = string> {
	readonly key: K | null;
	readonly dir: SortDir;
	toggle(key: K): void;
}

const empty = (v: SortValue) => v == null || v === '' || (typeof v === 'number' && Number.isNaN(v));

function compare(a: SortValue, b: SortValue): number {
	const ea = empty(a);
	const eb = empty(b);
	if (ea || eb) return ea === eb ? 0 : ea ? 1 : -1;
	if (typeof a === 'number' && typeof b === 'number') return a - b;
	if (typeof a === 'boolean' && typeof b === 'boolean') return Number(a) - Number(b);
	if (a instanceof Date && b instanceof Date) return a.getTime() - b.getTime();
	return String(a).localeCompare(String(b), undefined, { numeric: true, sensitivity: 'base' });
}

export class TableSort<T, K extends string = string> implements SortLike<K> {
	key = $state<K | null>(null);
	dir = $state<SortDir>('asc');

	constructor(
		readonly columns: Record<K, SortColumn<T>>,
		initial?: { key: K; dir?: SortDir }
	) {
		if (initial) {
			this.key = initial.key;
			this.dir = initial.dir ?? columns[initial.key].dir ?? 'asc';
		}
	}

	/** natural → reversed → off (the table's own order) */
	toggle(key: K) {
		const first = this.columns[key].dir ?? 'asc';
		if (this.key !== key) {
			this.key = key;
			this.dir = first;
		} else if (this.dir === first) {
			this.dir = first === 'asc' ? 'desc' : 'asc';
		} else {
			this.key = null;
			this.dir = 'asc';
		}
	}

	/** a sorted copy, or the rows as given when no column is active. Stable, so ties keep their order. */
	sorted(rows: T[]): T[] {
		const key = this.key;
		if (key === null) return rows;
		const by = this.columns[key].by;
		const sign = this.dir === 'asc' ? 1 : -1;
		// empties stay last whichever way the column points
		return rows
			.map((row, i) => ({ row, i, v: by(row) }))
			.sort((x, y) => {
				const ex = empty(x.v);
				const ey = empty(y.v);
				if (ex || ey) return ex === ey ? x.i - y.i : ex ? 1 : -1;
				return sign * compare(x.v, y.v) || x.i - y.i;
			})
			.map((r) => r.row);
	}
}

/** Case-insensitive substring match of a search box against any of the given fields. */
export function matches(needle: string, ...fields: SortValue[]): boolean {
	const q = needle.trim().toLowerCase();
	if (!q) return true;
	return fields.some((f) => f != null && String(f).toLowerCase().includes(q));
}
