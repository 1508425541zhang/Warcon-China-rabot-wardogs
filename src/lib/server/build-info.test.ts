import { afterAll, describe, expect, test } from 'bun:test';
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { buildInfo, parseCommit, readGitHead, resolveCommit } from './build-info';

const HASH = '23ebacb0c1d2e3f4a5b6c7d8e9f0a1b2c3d4e5f6';
const OTHER = 'aaaaaaa0c1d2e3f4a5b6c7d8e9f0a1b2c3d4e5f6';
const dirs: string[] = [];
function checkout(files: Record<string, string>): string {
	const root = mkdtempSync(join(tmpdir(), 'warcon-build-'));
	dirs.push(root);
	for (const [path, text] of Object.entries(files)) {
		mkdirSync(join(root, path, '..'), { recursive: true });
		writeFileSync(join(root, path), text);
	}
	return root;
}
afterAll(() => dirs.forEach((d) => rmSync(d, { recursive: true, force: true })));

describe('parseCommit', () => {
	test('shortens a full hash and keeps the dirty mark', () => {
		expect(parseCommit(`${HASH.toUpperCase()}\n`)).toBe('23ebacb');
		expect(parseCommit('23ebacb-dirty')).toBe('23ebacb-dirty');
	});
	test('anything that is not a hash reads as unknown', () => {
		for (const raw of [undefined, '', 'main', 'abc12', '<script>alert(1)</script>', '23ebacb; rm'])
			expect(parseCommit(raw)).toBe('');
	});
});

describe('readGitHead', () => {
	test('a detached HEAD, a loose ref and a packed ref', () => {
		expect(readGitHead(checkout({ '.git/HEAD': `${HASH}\n` }))).toBe('23ebacb');
		expect(
			readGitHead(
				checkout({ '.git/HEAD': 'ref: refs/heads/main\n', '.git/refs/heads/main': `${HASH}\n` })
			)
		).toBe('23ebacb');
		expect(
			readGitHead(
				checkout({
					'.git/HEAD': 'ref: refs/heads/fix/x\n',
					'.git/packed-refs': `# pack-refs with: peeled\n${OTHER} refs/heads/main\n${HASH} refs/heads/fix/x\n`
				})
			)
		).toBe('23ebacb');
	});
	test('no checkout, or a ref that points outside refs/heads, reads as unknown', () => {
		expect(readGitHead(checkout({}))).toBe('');
		expect(
			readGitHead(checkout({ '.git/HEAD': 'ref: refs/heads/../../../secret\n', secret: HASH }))
		).toBe('');
	});
});

describe('resolveCommit', () => {
	test('the environment wins, then the checkout, then the file the build wrote', () => {
		const both = checkout({ '.git/HEAD': HASH, 'build/commit': OTHER });
		expect(resolveCommit(both, `${OTHER}-dirty`)).toBe('aaaaaaa-dirty');
		expect(resolveCommit(both, '')).toBe('23ebacb');
		expect(resolveCommit(checkout({ 'build/commit': OTHER }), '')).toBe('aaaaaaa');
		expect(resolveCommit(checkout({}), 'not a hash')).toBe('');
	});
});

test('this checkout reports its package version and commit', () => {
	const b = buildInfo();
	expect(b.version).toMatch(/^\d+\.\d+\.\d+/);
	expect(b.commit).toMatch(/^([0-9a-f]{7}(-dirty)?)?$/);
});
