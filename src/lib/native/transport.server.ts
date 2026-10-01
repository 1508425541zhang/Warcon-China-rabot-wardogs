/** Rendering transport only. Rust owns identities, authorization, validation and persistence. */
import { env } from '$env/dynamic/private';
import { createHmac } from 'node:crypto';
import { error, fail, redirect, type Actions, type RequestEvent } from '@sveltejs/kit';
import type { PageContracts } from './page-contracts';
import type { Wire } from './types';
import QRCode from 'qrcode';

type NativeFailure = {
	error?: string;
	message?: string;
	username?: string;
	displayName?: string;
	orgName?: string;
	totp?: { secret: string; uri: string; svg: string };
	totpRetry?: boolean;
	backupCodes?: string[];
	recoveryKey?: string;
	defaultOrg?: boolean;
	steam?: boolean;
	changed?: boolean;
	set?: boolean;
	removed?: boolean;
	recoveryKeyCleared?: boolean;
	revoked?: boolean;
	passwordRemoved?: boolean;
	totpDisabled?: boolean;
	totpEnabled?: boolean;
	unlinked?: boolean;
};
type Envelope<T> = {
	data: T;
	redirect?: string;
	failed?: boolean;
	status?: number;
	error?: { message?: string; code?: string };
};
const HOP_HEADERS = [
	'connection',
	'keep-alive',
	'proxy-authenticate',
	'proxy-authorization',
	'te',
	'trailer',
	'transfer-encoding',
	'upgrade',
	'host',
	'content-length'
];

export function backendUrl(path: string): string {
	const base = new URL(env.RUST_BACKEND_URL || 'http://127.0.0.1:4300');
	if (
		!['http:', 'https:'].includes(base.protocol) ||
		base.username ||
		base.password ||
		base.search ||
		base.hash ||
		base.pathname !== '/'
	) {
		throw new Error('RUST_BACKEND_URL must be a fixed HTTP(S) service origin.');
	}
	if (!path.startsWith('/') || path.startsWith('//')) throw new Error('Invalid native route.');
	base.pathname = path.split('?')[0];
	base.search = path.includes('?') ? path.slice(path.indexOf('?') + 1) : '';
	return base.href;
}
export function nativeHeaders(event: RequestEvent): Headers {
	const headers = new Headers(event.request.headers);
	for (const key of HOP_HEADERS) headers.delete(key);
	for (const key of [
		'x-warcon-client-ip',
		'x-warcon-client-time',
		'x-warcon-client-proof',
		'x-forwarded-for',
		'forwarded',
		'x-real-ip'
	])
		headers.delete(key);
	const secret = env.RUST_FRONTEND_TOKEN;
	if (secret) {
		if (secret.length < 32)
			throw new Error('RUST_FRONTEND_TOKEN must contain at least 32 characters.');
		let address: string;
		try {
			address = event.getClientAddress();
		} catch {
			address = '127.0.0.1';
		}
		const stamp = String(Math.floor(Date.now() / 1000));
		headers.set('x-warcon-client-ip', address);
		headers.set('x-warcon-client-time', stamp);
		headers.set(
			'x-warcon-client-proof',
			createHmac('sha256', secret).update(`${stamp}\n${address}`).digest('hex')
		);
	}
	headers.set('accept-encoding', 'identity');
	return headers;
}
export function applyCookies(event: RequestEvent, headers: Headers): void {
	for (const raw of headers.getSetCookie()) {
		const [pair, ...attributes] = raw.split(';');
		const eq = pair.indexOf('=');
		if (eq <= 0) continue;
		const options: Parameters<typeof event.cookies.set>[2] = {
			path: '/',
			httpOnly: false,
			secure: false,
			sameSite: undefined
		};
		for (const item of attributes) {
			const [key, ...rest] = item.trim().split('=');
			const value = rest.join('=');
			switch (key.toLowerCase()) {
				case 'path':
					options.path = value || '/';
					break;
				case 'domain':
					options.domain = value;
					break;
				case 'secure':
					options.secure = true;
					break;
				case 'httponly':
					options.httpOnly = true;
					break;
				case 'samesite':
					if (['strict', 'lax', 'none'].includes(value.toLowerCase()))
						options.sameSite = value.toLowerCase() as 'strict' | 'lax' | 'none';
					break;
				case 'max-age':
					if (/^-?\d+$/.test(value)) options.maxAge = Number(value);
					break;
				case 'expires': {
					const date = new Date(value);
					if (Number.isFinite(+date)) options.expires = date;
					break;
				}
			}
		}
		let value = pair.slice(eq + 1);
		try {
			value = decodeURIComponent(value);
		} catch {
			/* An invalid literal cannot become a signed session. */
		}
		event.cookies.set(pair.slice(0, eq), value, options);
	}
}
async function request(event: RequestEvent, path: string, init: RequestInit): Promise<Response> {
	try {
		return await fetch(backendUrl(path), {
			...init,
			headers: init.headers ?? nativeHeaders(event),
			redirect: 'manual',
			signal: AbortSignal.any([event.request.signal, AbortSignal.timeout(70_000)])
		});
	} catch {
		error(503, 'Rust 后端暂时无法连接，请检查本地后端服务。');
	}
}
export async function nativeJson<T>(event: RequestEvent, path: string, body?: unknown): Promise<T> {
	const headers = nativeHeaders(event);
	headers.delete('authorization');
	headers.set('content-type', 'application/json');
	const response = await request(event, path, {
		method: body === undefined ? 'GET' : 'POST',
		headers,
		body: body === undefined ? undefined : JSON.stringify(body)
	});
	applyCookies(event, response.headers);
	let value: T & { error?: { message?: string } };
	try {
		value = await response.json();
	} catch {
		error(502, 'Rust 后端返回了无法解析的数据。');
	}
	if (!response.ok) error(response.status, value.error?.message || '请求失败。');
	return value;
}
function unwrap<T>(value: Envelope<T>): T {
	if (value.redirect)
		redirect((value.status ?? 303) as 301 | 302 | 303 | 307 | 308, value.redirect);
	return value.data;
}
export async function pageLoad<K extends keyof PageContracts>(
	event: RequestEvent,
	source: K
): Promise<Wire<PageContracts[K]>> {
	return unwrap(
		await nativeJson<Envelope<Wire<PageContracts[K]>>>(event, '/api/page/load', {
			source,
			params: event.params,
			search: event.url.searchParams.toString()
		})
	);
}
export function pageActions<K extends keyof PageContracts>(
	source: K,
	names: readonly string[]
): Actions {
	return Object.fromEntries(
		names.map((action) => [
			action,
			async (event: RequestEvent) => {
				const form: Record<string, string | string[]> = {};
				for (const [key, value] of await event.request.formData()) {
					if (typeof value !== 'string') error(400, '此表单不接受文件。');
					const before = form[key];
					form[key] =
						before === undefined
							? value
							: Array.isArray(before)
								? [...before, value]
								: [before, value];
				}
				const result = await nativeJson<Envelope<NativeFailure>>(event, '/api/page/action', {
					source,
					action,
					params: event.params,
					search: event.url.searchParams.toString(),
					form
				});
				const data = unwrap(result);
				// QR rendering is presentation; OTP state and verification belong to Rust.
				if (data?.totp?.uri)
					data.totp.svg = await QRCode.toString(data.totp.uri, { type: 'svg', margin: 2 });
				return result.failed ? fail(result.status ?? 400, data) : data;
			}
		])
	);
}
export async function nativeProxy(
	event: RequestEvent,
	path = event.url.pathname + event.url.search
): Promise<Response> {
	const headers = nativeHeaders(event),
		method = event.request.method;
	const controller = new AbortController();
	const timer = setTimeout(() => controller.abort(), 70_000);
	let response: Response;
	try {
		response = await fetch(backendUrl(path), {
			method,
			headers,
			redirect: 'manual',
			body: method === 'GET' || method === 'HEAD' ? undefined : event.request.body,
			signal: AbortSignal.any([event.request.signal, controller.signal]),
			duplex: 'half'
		} as RequestInit);
	} catch {
		return Response.json(
			{ ok: false, error: { code: 'native_unavailable', message: 'Rust 后端暂时无法连接。' } },
			{ status: 503 }
		);
	} finally {
		clearTimeout(timer);
	}
	const outgoing = new Headers(response.headers);
	for (const key of HOP_HEADERS) outgoing.delete(key);
	return new Response(response.body, {
		status: response.status,
		statusText: response.statusText,
		headers: outgoing
	});
}
