// Which build this process is running: the package version and the commit it was built from, for
// the Admin overview and /metrics, so an install on an old build can be told apart at a glance.
// Nobody has to do anything for it to be right: a run from a checkout reads .git, and the build
// writes what it read to build/commit for the image, which has no .git. WARCON_COMMIT overrides
// both, for a build whose source arrives without .git (a build argument in the Dockerfile).
// Files only: nothing is run. Neither value is a secret, but both are shown only where the rest
// of the process figures are.
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { version } from '../../../package.json';

export interface BuildInfo {
	version: string;
	/** seven hex characters, "-dirty" after them when the deploy says so; '' when unknown */
	commit: string;
}

const COMMIT = /^([0-9a-f]{7,40})(-dirty)?$/;
/** the file the build leaves beside its output, under the working directory */
export const COMMIT_FILE = 'build/commit';

/** A commit as it is shown: only a hex hash is accepted, wherever the value came from. */
export function parseCommit(raw: string | undefined): string {
	const m = COMMIT.exec((raw ?? '').trim().toLowerCase());
	return m ? m[1].slice(0, 7) + (m[2] ?? '') : '';
}

const read = (path: string) => {
	try {
		return readFileSync(path, 'utf8');
	} catch {
		return '';
	}
};

/** HEAD of the checkout at `root`, from .git's own files: a detached hash, a loose ref or a packed one. */
export function readGitHead(root: string): string {
	const head = read(join(root, '.git/HEAD')).trim();
	const ref = /^ref: (refs\/heads\/[\w./-]+)$/.exec(head)?.[1];
	if (!ref || ref.includes('..')) return parseCommit(head);
	const packed = read(join(root, '.git/packed-refs'))
		.split('\n')
		.find((line) => line.endsWith(` ${ref}`));
	return parseCommit(read(join(root, '.git', ref)) || packed?.split(' ')[0]);
}

/** Environment first, then the checkout, then what the build wrote down. */
export function resolveCommit(root: string, env = process.env.WARCON_COMMIT): string {
	return parseCommit(env) || readGitHead(root) || parseCommit(read(join(root, COMMIT_FILE)));
}

let cached: BuildInfo | null = null;

export function buildInfo(): BuildInfo {
	cached ??= { version, commit: resolveCommit(process.cwd()) };
	return cached;
}
