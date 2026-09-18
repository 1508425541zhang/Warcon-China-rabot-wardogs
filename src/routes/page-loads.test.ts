import { describe, expect, test } from 'bun:test';
import { readdirSync, readFileSync } from 'node:fs';
import { join, relative } from 'node:path';

// A page's data can be requested without its layouts: SvelteKit's __data.json takes a mask of the
// nodes to run, and even without the mask a layout's refusal and the page's data are answered
// side by side. So a +layout.server.ts check protects nothing below it, and every page load and
// form action under (app) has to check access itself. This reads the sources: it cannot prove a
// check is the right one, but it fails when the check a route family needs is missing, which is
// how the admin overview and three server pages came to answer anyone.

const APP = join(import.meta.dir, '(app)');

const pages = (dir: string): string[] =>
	readdirSync(dir, { withFileTypes: true }).flatMap((e) =>
		e.isDirectory()
			? pages(join(dir, e.name))
			: e.name === '+page.server.ts'
				? [join(dir, e.name)]
				: []
	);

/** What a page has to call before it reads anything, by where it lives. */
const FAMILIES: { under: string; needs: RegExp; says: string }[] = [
	{ under: 'admin/', needs: /role !== 'owner'|requireOwner\(/, says: 'the site owner check' },
	{
		under: 'server/[id]/',
		needs: /requireServerCap\(|requireServerManager\(|orgRoleFor\(|await parent\(\)|parent\(\),/,
		says: 'requireServerCap (or the layout through parent())'
	},
	{
		under: 'orgs/[id]/',
		needs: /requireOrgRole\(|requireListsRole\(/,
		says: 'requireOrgRole or requireListsRole'
	},
	{ under: '', needs: /requireUser\(|await parent\(\)/, says: 'requireUser' }
];

describe('page loads under (app)', () => {
	const files = pages(APP);

	test('there are pages to check', () => {
		expect(files.length).toBeGreaterThan(10);
	});

	for (const file of files) {
		const name = relative(APP, file);
		const source = readFileSync(file, 'utf8');
		// A page that only redirects reads nothing and has nothing to guard.
		if (!source.includes('$lib/server/')) continue;
		const family = FAMILIES.find((f) => name.startsWith(f.under))!;
		test(`${name} checks access itself (${family.says})`, () => {
			expect(family.needs.test(source)).toBe(true);
		});
	}
});
