import { describe, expect, test } from 'bun:test';
import { readdirSync, readFileSync } from 'node:fs';
import { join, relative } from 'node:path';

// Rust tests verify independent page authorization. This check catches reactivation of the old
// database/workers in the renderer or disappearance of a registered page/action.
const root = join(import.meta.dir, '../..');
const inventory: { source: string; actions: string[] }[] = JSON.parse(
	readFileSync(join(root, 'backend/assets/page-inventory.json'), 'utf8')
);
function pages(folder: string): string[] {
	return readdirSync(folder, { withFileTypes: true }).flatMap((entry) =>
		entry.isDirectory()
			? pages(join(folder, entry.name))
			: /^\+(page|layout)\.server\.ts$/.test(entry.name)
				? [relative(root, join(folder, entry.name)).replaceAll('\\', '/')]
				: []
	);
}
describe('native renderer boundary', () => {
	test('all existing server page contracts are still reachable', () => {
		expect(pages(join(root, 'src/routes')).sort()).toEqual(inventory.map((p) => p.source).sort());
	});
	for (const page of inventory) {
		test(page.source, () => {
			const source = readFileSync(join(root, page.source), 'utf8');
			expect(source.match(/pageLoad\(\s*event,\s*['"]([^'"]+)['"]\s*\)/)?.[1]).toBe(page.source);
			expect(source).not.toMatch(/\$lib\/server\/|drizzle|better-auth|Bun\./);
			if (page.actions.length) {
				const names = source.match(/pageActions\([^,]+,\s*(\[[\s\S]*?\])/);
				expect(names).not.toBeNull();
				expect([...names![1].matchAll(/['"]([^'"]+)['"]/g)].map((m) => m[1])).toEqual(page.actions);
			}
		});
	}
	test('request hooks cannot initialize a legacy backend', () => {
		const source = readFileSync(join(root, 'src/hooks.server.ts'), 'utf8');
		expect(source).toContain('nativeProxy(event)');
		expect(source).toContain("'/api/identity/session'");
		expect(source).not.toMatch(/\$lib\/server\/|startPoller|startWorker|better-auth|drizzle|Bun\./);
	});
});
