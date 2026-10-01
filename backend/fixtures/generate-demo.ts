// Offline source fixture. The native runtime never evaluates TypeScript.
import { createHash, webcrypto } from 'node:crypto';
import { rotationFromText } from '../../src/lib/rotation-doc';
import {
	hasReservedKey,
	reservedFromText,
	reservedIntoText,
	reservedSlotsHeld
} from '../../src/lib/reserved-doc';
const source = await Bun.file(new URL('../../src/lib/server/mockgame.ts', import.meta.url)).text();
const fixed = 1780000000000;
class FixtureDate extends Date {
	constructor(value?: string | number | Date) {
		super(value === undefined ? fixed : value instanceof Date ? value.getTime() : value);
	}
	static now() {
		return fixed;
	}
}
const script = new Bun.Transpiler({ loader: 'ts' }).transformSync(
	source.replace(/^import[\s\S]*?;\r?\n/gm, '').replace(/^export /gm, '')
);
const read = new Function(
	'createHash',
	'rotationFromText',
	'hasReservedKey',
	'reservedFromText',
	'reservedIntoText',
	'reservedSlotsHeld',
	'crypto',
	'Date',
	script +
		'\nreturn { seed, MAPS, EXPERIENCES, EXPERIENCES_BY_MAP, LIGHTINGS, ALTERNATORS_BY_MAP, JOINERS, FEED_CAUSES, CAPABILITY_ROUTES, LIVE_BUILD_MISSING: [...LIVE_BUILD_MISSING], CONFIG_SECTIONS };'
);
const sourceDemo = read(
	createHash,
	rotationFromText,
	hasReservedKey,
	reservedFromText,
	reservedIntoText,
	reservedSlotsHeld,
	webcrypto,
	FixtureDate
);
const { seed, ...catalog } = sourceDemo;
await Bun.write(
	new URL('../assets/demo.json', import.meta.url),
	JSON.stringify({ fixed, seed: seed('fixture'), ...catalog }, null, 2) + '\n'
);
