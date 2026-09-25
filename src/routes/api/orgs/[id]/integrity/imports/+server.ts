import { getEnv } from '$lib/server/env';
import { requireOrgRole } from '$lib/server/access';
import { ApiError, apiJson, param, route } from '$lib/server/http';
import {
	integrityImports,
	MAX_IMPORT_BYTES,
	stageIntegrityImport
} from '$lib/server/integrity/imports';

export const GET = route(async (event) => {
	const env = getEnv();
	const { org } = await requireOrgRole(env, event.locals, param(event, 'id'), 'owner');
	return apiJson({ ok: true, batches: await integrityImports(env, org.id) });
});

export const POST = route(async (event) => {
	const env = getEnv();
	const { org, user } = await requireOrgRole(env, event.locals, param(event, 'id'), 'owner');
	const contentLength = Number(event.request.headers.get('content-length') ?? 0);
	if (contentLength > MAX_IMPORT_BYTES + 100_000) throw new ApiError(413, '上传文件超过 8 MiB。');
	if (!event.request.headers.get('content-type')?.startsWith('multipart/form-data'))
		throw new ApiError(415, '请使用 multipart/form-data 上传文件。');
	let form: FormData;
	try {
		form = await event.request.formData();
	} catch {
		throw new ApiError(400, '上传表单格式无效。');
	}
	const file = form.get('file');
	const sourceServer = form.get('sourceServer');
	if (!(file instanceof File) || file.size > MAX_IMPORT_BYTES)
		throw new ApiError(400, '请选择小于 8 MiB 的 JSON 或 JSONL 文件。');
	if (!/\.jsonl?$/i.test(file.name)) throw new ApiError(400, '文件扩展名必须是 .json 或 .jsonl。');
	if (typeof sourceServer !== 'string') throw new ApiError(400, '请填写来源服务器标识。');
	return apiJson(
		{
			ok: true,
			batch: await stageIntegrityImport(
				env,
				event.request,
				user,
				org.id,
				sourceServer.trim(),
				await file.text()
			)
		},
		201
	);
});
