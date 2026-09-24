import { expect, test } from 'bun:test';
import { evidenceConfidence } from './evidence';

test('only a complete, unambiguous trigger-event snapshot earns B', () => {
	expect(evidenceConfidence(['a', 'b'], ['b', 'a'])).toBe('B');
	expect(evidenceConfidence(['a', 'b'], ['a'])).toBe('C');
	expect(evidenceConfidence(['a', 'b'], ['a', 'a'])).toBe('C');
	expect(evidenceConfidence(['a', 'b'], [])).toBe('D');
});
