// HTTP client for the game server's WDRCON listener: plain fetch to http(s)://host:port.
import { isIP } from 'node:net';

export interface GameTarget {
	host: string;
	port: number;
	scheme: 'http' | 'https';
	/**
	 * The IPs the host policy resolved and approved, best family first. When set, the request
	 * connects to these addresses rather than resolving `host` a second time, so a name that passed
	 * the check cannot rebind to an internal address before the socket opens; the addresses are tried
	 * in order (as the OS would with happy-eyeballs, and all already validated) until one connects.
	 * The hostname still travels as the Host header and the TLS server name, so virtual hosts and
	 * certificates keep working.
	 */
	addresses?: string[];
}

/** Wrap an IPv6 literal in brackets for a URL authority; leave a hostname or IPv4 as is. */
const forAuthority = (addr: string): string => (isIP(addr) === 6 ? `[${addr}]` : addr);

export interface GameRequestInit {
	method: string;
	path: string;
	headers?: Record<string, string>;
	body?: string;
	timeoutMs?: number;
	/** Accept self-signed certificates (Bun's `tls` fetch option). */
	insecureTls?: boolean;
}

export interface GameResponse {
	status: number;
	statusText: string;
	headers: Record<string, string>;
	text: string;
}

export class TransportError extends Error {
	constructor(
		message: string,
		public cause?: unknown
	) {
		super(message);
	}
}

export async function gameRequest(
	target: GameTarget,
	init: GameRequestInit
): Promise<GameResponse> {
	const bareHost = target.host.replace(/^\[|\]$/g, '');
	// The validated addresses to connect to, or the host itself when none were pinned (a bare-IP
	// target, or a caller that did not resolve). All attempts share one timeout budget.
	const addresses = target.addresses?.length ? target.addresses : [bareHost];
	const signal = AbortSignal.timeout(init.timeoutMs ?? 10000);
	let lastErr: unknown;
	for (let i = 0; i < addresses.length; i++) {
		const addr = addresses[i];
		// A pinned address is one that is not the host text itself (a literal IP target pins to
		// itself and needs no rewrite). Then the hostname stays in the URL's Host and, over TLS, as
		// the server name, so nothing the game server sees changes.
		const pinned = addr !== bareHost;
		const url = pinned
			? `${target.scheme}://${forAuthority(addr)}:${target.port}${init.path}`
			: `${target.scheme}://${target.host}:${target.port}${init.path}`;
		const request: BunFetchRequestInit = {
			method: init.method,
			headers: {
				'user-agent': 'warcon/0.3',
				accept: 'application/json, text/plain, */*',
				...init.headers,
				...(pinned ? { host: `${target.host}:${target.port}` } : {})
			},
			body: init.body,
			signal,
			redirect: 'manual'
		};
		if (init.insecureTls || (pinned && target.scheme === 'https'))
			request.tls = {
				...(init.insecureTls ? { rejectUnauthorized: false } : {}),
				// Validate the certificate against the hostname, not the pinned IP.
				...(pinned && target.scheme === 'https' ? { serverName: bareHost } : {})
			};
		try {
			const res = await fetch(url, request);
			const headers: Record<string, string> = {};
			res.headers.forEach((v, k) => {
				headers[k.toLowerCase()] = v;
			});
			return { status: res.status, statusText: res.statusText, headers, text: await res.text() };
		} catch (err) {
			lastErr = err;
			// The next validated address is tried only when this one never opened (wrong family,
			// refused): never a fresh resolution, so the connection can only land on an approved
			// address. Anything later may have reached the game, which may have acted on it; sending
			// it again would end a match or kick a player twice, so that is a failure the caller sees.
			const name = (err as { name?: string }).name;
			const done = signal.aborted || name === 'TimeoutError' || name === 'AbortError';
			if (done || !neverOpened(err) || i === addresses.length - 1) {
				const cause = (err as { cause?: { code?: string; message?: string } }).cause;
				const detail = cause?.code || cause?.message || (err as Error).message;
				throw new TransportError(
					`Could not reach the game server (${withoutTarget(detail, target, addresses)}).`,
					err
				);
			}
		}
	}
	throw new TransportError('Could not reach the game server.', lastErr);
}

/** Bun's codes, and Node's, for a connection that failed before anything was sent on it. */
const NEVER_OPENED = new Set([
	'ConnectionRefused',
	'FailedToOpenSocket',
	'ECONNREFUSED',
	'EHOSTUNREACH',
	'ENETUNREACH',
	'EADDRNOTAVAIL'
]);

function neverOpened(err: unknown): boolean {
	const e = err as { code?: string; cause?: { code?: string } };
	return NEVER_OPENED.has(e.code ?? '') || NEVER_OPENED.has(e.cause?.code ?? '');
}

/**
 * The message is stored as the server's live error and shown to everyone who can open the server,
 * and on its Discord card, so it never names where RCON listens: that is for the org's owners,
 * who have it on the server's form.
 */
function withoutTarget(detail: string, target: GameTarget, addresses: string[]): string {
	let out = detail;
	for (const part of [target.host, target.host.replace(/^\[|\]$/g, ''), ...addresses])
		if (part) out = out.split(part).join('the address');
	return out;
}
