import { request } from 'node:https';
import { isIP } from 'node:net';
import { assertReachableTarget, pinnedAddresses } from '../hostpolicy';
import { ApiError } from '../http';
import { apiBase } from './ai-protocol';
/** Fixed resolved address, TLS verification, no redirects, bounded response; no secret-bearing errors. */
export async function aiRequest(
	base: string,
	key: string,
	path: 'models' | 'chat/completions',
	body?: unknown
): Promise<unknown> {
	const url = new URL(`${apiBase(base)}/${path}`);
	const addresses = pinnedAddresses(url.hostname, await assertReachableTarget(url.hostname, false));
	return new Promise((resolve, reject) => {
		const req = request(
			url,
			{
				method: body === undefined ? 'GET' : 'POST',
				headers: {
					authorization: `Bearer ${key}`,
					'content-type': 'application/json',
					accept: 'application/json'
				},
				lookup: (_host, _options, cb) => cb(null, addresses[0], isIP(addresses[0]))
			},
			(res) => {
				const chunks: Buffer[] = [];
				let bytes = 0;
				res.on('data', (chunk: Buffer) => {
					bytes += chunk.length;
					if (bytes > 1024 * 1024) req.destroy(new Error('response_limit'));
					else chunks.push(chunk);
				});
				res.on('error', () => reject(new ApiError(502, '模型响应中断，请稍后重试。')));
				res.on('end', () => {
					clearTimeout(timer);
					if (!res.statusCode || res.statusCode < 200 || res.statusCode >= 300)
						return reject(
							new ApiError(
								502,
								`模型接口返回 HTTP ${res.statusCode}。请检查地址、权限、模型名称或额度。`
							)
						);
					try {
						resolve(JSON.parse(Buffer.concat(chunks).toString('utf8')));
					} catch {
						reject(new ApiError(502, '模型接口没有返回有效 JSON。'));
					}
				});
			}
		);
		const timer = setTimeout(() => req.destroy(new Error('timeout')), 60000);
		req.on('error', () => {
			clearTimeout(timer);
			reject(new ApiError(502, '模型连接失败、超时或响应过大；请检查服务商配置。'));
		});
		req.end(body === undefined ? undefined : JSON.stringify(body));
	});
}
