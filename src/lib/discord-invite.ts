// An organisation's Discord invite link, as shown on its public pages. Only an invite URL is
// accepted (discord.gg/<code> or discord.com/invite/<code>), so the button can never point
// anywhere else. Client-safe, so the form can validate as the owner types.

const CODE = /^[A-Za-z0-9-]{2,64}$/;

/** The canonical https://discord.gg/<code> form of an invite link, or null when it is not one. */
export function parseDiscordInvite(raw: string): string | null {
	const text = raw.trim();
	if (!text) return null;
	let u: URL;
	try {
		u = new URL(/^https?:\/\//i.test(text) ? text : `https://${text}`);
	} catch {
		return null;
	}
	const host = u.hostname.toLowerCase();
	const parts = u.pathname.split('/').filter(Boolean);
	let code: string | undefined;
	if (host === 'discord.gg') code = parts.length === 1 ? parts[0] : undefined;
	else if (host === 'discord.com' || host === 'www.discord.com' || host === 'discordapp.com')
		code = parts.length === 2 && parts[0] === 'invite' ? parts[1] : undefined;
	if (!code || !CODE.test(code)) return null;
	return `https://discord.gg/${code}`;
}
