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
		// One check somewhere in the file is not enough for a form action: it runs no load and no
		// layout, and there is no parent() to lean on, so each action carries the check itself.
		for (const [action, body] of actionsOf(source))
			test(`${name} action '${action}' checks access itself (${family.says})`, () => {
				expect(family.needs.test(body.replace(/await parent\(\)|parent\(\),/g, ''))).toBe(true);
			});
	}
});

/** The entries of `export const actions`, each with its source, split where an entry begins. */
function actionsOf(source: string): [string, string][] {
	const at = source.indexOf('export const actions');
	if (at < 0) return [];
	const parts = source.slice(at).split(/^\t(\w+): async /m);
	const out: [string, string][] = [];
	// An action may hand straight over to a function of the same file: its source counts as the action's.
	const helper = (name: string) =>
		source.match(new RegExp(`^(?:async )?function ${name}\\([\\s\\S]*?^}`, 'm'))?.[0] ?? '';
	for (let i = 1; i < parts.length; i += 2) {
		const body = parts[i + 1].split(/^};?$/m)[0];
		const called = [...body.matchAll(/\b(\w+)\(/g)].map((m) => helper(m[1]));
		out.push([parts[i], body + called.join('')]);
	}
	return out;
}
