import { describe, expect, test } from 'bun:test';
import { parseDiscordInvite } from './discord-invite';

describe('parseDiscordInvite', () => {
	test('accepts the two invite forms and returns the short one', () => {
		expect(parseDiscordInvite('https://discord.gg/abc123')).toBe('https://discord.gg/abc123');
		expect(parseDiscordInvite('https://discord.com/invite/abc123')).toBe(
			'https://discord.gg/abc123'
		);
		expect(parseDiscordInvite('discord.gg/warcon-eu')).toBe('https://discord.gg/warcon-eu');
		expect(parseDiscordInvite('  https://discord.gg/abc123/  ')).toBe('https://discord.gg/abc123');
	});
	test('refuses anything that is not an invite', () => {
		for (const bad of [
			'',
			'https://example.com/invite/abc',
			'https://discord.com/channels/1/2',
			'https://discord.gg/',
			'https://discord.gg/a/b',
			'https://discord.com/abc',
			'javascript:alert(1)',
			'https://discord.gg/has space'
		])
			expect(parseDiscordInvite(bad)).toBeNull();
	});
});
