import { and, asc, desc, eq, or, isNull, lt, sql } from 'drizzle-orm';
import { createHash } from 'node:crypto';
import type { Env } from '../env';
import {
	integrityAiSettings,
	integrityAiReviews,
	integrityCases,
	integrityCaseEvents,
	integrityLabels,
	integrityActions,
	steamProfiles
} from '../db/schema';
import { encryptSecret, decryptSecret } from '../crypto';
import { ApiError } from '../http';
import {
	apiBase,
	settingsInput,
	reviewOutput,
	numericChecks,
	SYSTEM,
	PROMPT_VERSION
} from './ai-protocol';
import { aiRequest } from './ai-client';
import { expandedAiEvidence } from './ai-evidence';
export async function aiSettings(env: Env, orgId: string) {
	const [row] = await env.db
		.select()
		.from(integrityAiSettings)
		.where(eq(integrityAiSettings.orgId, orgId));
	return row;
}
export async function saveAiSettings(env: Env, orgId: string, input: unknown) {
	const parsed = settingsInput.safeParse(input);
	if (!parsed.success)
		throw new ApiError(400, '配置格式不正确：请检查地址、模型和输出上限（256–4096）。');
	const value = parsed.data,
		old = await aiSettings(env, orgId);
	const baseUrl = apiBase(value.baseUrl);
	if ((!old || old.baseUrl !== baseUrl) && !value.apiKey)
		throw new ApiError(400, '首次配置或更换 API 地址时必须重新填写密钥。');
	if (/[\r\n]/.test(value.apiKey)) throw new ApiError(400, '密钥不能包含换行。');
	const row = {
		orgId,
		autoEnabled: value.autoEnabled,
		dailyLimit: value.dailyLimit,
		baseUrl,
		model: value.model,
		tokenParameter: value.tokenParameter,
		maxTokens: value.maxTokens,
		keyEnc: value.apiKey ? encryptSecret(env, value.apiKey) : old!.keyEnc,
		updatedAt: new Date()
	};
	await env.db
		.insert(integrityAiSettings)
		.values(row)
		.onConflictDoUpdate({ target: integrityAiSettings.orgId, set: row });
}
export async function aiBundle(env: Env, orgId: string, serverId: string, caseId: string) {
	const [c] = await env.db
		.select()
		.from(integrityCases)
		.where(
			and(
				eq(integrityCases.id, caseId),
				eq(integrityCases.orgId, orgId),
				eq(integrityCases.serverId, serverId)
			)
		);
	if (!c) throw new ApiError(404, '案件不存在或无权查看。');
	const [events, reviews, actions, history] = await Promise.all([
		env.db
			.select({
				instanceId: integrityCaseEvents.instanceId,
				eventId: integrityCaseEvents.eventId,
				event: integrityCaseEvents.event
			})
			.from(integrityCaseEvents)
			.where(eq(integrityCaseEvents.caseId, c.id))
			.orderBy(asc(integrityCaseEvents.instanceId), asc(integrityCaseEvents.eventId)),
		env.db
			.select({
				label: integrityLabels.label,
				reason: integrityLabels.reason,
				createdAt: integrityLabels.createdAt
			})
			.from(integrityLabels)
			.where(and(eq(integrityLabels.caseId, c.id), eq(integrityLabels.orgId, orgId)))
			.orderBy(asc(integrityLabels.id)),
		env.db
			.select()
			.from(integrityActions)
			.where(eq(integrityActions.caseId, c.id))
			.orderBy(asc(integrityActions.id)),
		env.db
			.select({
				id: integrityCases.id,
				createdAt: integrityCases.createdAt,
				riskScore: integrityCases.riskScore,
				status: integrityCases.status
			})
			.from(integrityCases)
			.where(and(eq(integrityCases.serverId, serverId), eq(integrityCases.steamId, c.steamId)))
			.orderBy(desc(integrityCases.createdAt))
			.limit(21)
	]);
	const [player] = await env.db
		.select({
			steamId: steamProfiles.steamId,
			name: steamProfiles.persona,
			fetchedAt: steamProfiles.fetchedAt
		})
		.from(steamProfiles)
		.where(eq(steamProfiles.steamId, c.steamId));
	return {
		player: player ?? { steamId: c.steamId, name: null },
		schemaVersion: 2,
		scope:
			'案件冻结资料＋玩家整局击杀死亡＋案件180秒全服交战背景；无法定位轮次时为案件前24小时玩家记录。逐条完整、无抽样；数据库记录完整不代表游戏回传无缺失。历史索引仍最多20条。',
		case: c,
		evidence: await expandedAiEvidence(env, c, events),
		reviews,
		actions,
		recentCases: history.slice(0, 20),
		historyTruncated: history.length > 20,
		numericChecks: numericChecks(c.snapshot),
		coverage: {
			savedEvents: events.length,
			expectedEventIds: (c.snapshot as { eventIds?: unknown })?.eventIds ?? null
		}
	};
}
export async function aiCall(
	env: Env,
	orgId: string,
	operation: 'models' | 'test' | 'review',
	bundle?: Awaited<ReturnType<typeof aiBundle>>,
	automatic = false,
	requester: typeof aiRequest = aiRequest
) {
	const config = await aiSettings(env, orgId);
	if (!config) throw new ApiError(400, '请先保存 API 地址和密钥。');
	if (automatic && !config.autoEnabled) throw new ApiError(409, '自动初审已暂停。');
	if (operation !== 'models' && !config.model) throw new ApiError(400, '请选择模型并保存。');
	const input = JSON.stringify(bundle);
	if (input && Buffer.byteLength(input) > 256000)
		throw new ApiError(413, '案件 JSON 超过 256 KB，本次未发送。请先导出核对；不会静默删减日志。');
	const fingerprint = createHash('sha256')
		.update(
			JSON.stringify([
				SYSTEM,
				config.baseUrl,
				config.model,
				config.maxTokens,
				config.tokenParameter,
				input
			])
		)
		.digest('hex');
	if (operation === 'review') {
		const [cached] = await env.db
			.select()
			.from(integrityAiReviews)
			.where(eq(integrityAiReviews.fingerprint, fingerprint));
		if (cached) return { result: cached.result, cached: true, createdAt: cached.createdAt };
	}
	const claimed = await env.db
		.update(integrityAiSettings)
		.set({
			lastRequestAt: new Date(),
			...(automatic
				? {
						budgetDay: new Date().toISOString().slice(0, 10),
						dailyRequests: sql`CASE WHEN ${integrityAiSettings.budgetDay} = ${new Date().toISOString().slice(0, 10)} THEN ${integrityAiSettings.dailyRequests} + 1 ELSE 1 END`
					}
				: {})
		})
		.where(
			and(
				eq(integrityAiSettings.orgId, orgId),
				eq(integrityAiSettings.updatedAt, config.updatedAt),
				...(automatic
					? [
							eq(integrityAiSettings.autoEnabled, true),
							sql`(${integrityAiSettings.budgetDay} <> ${new Date().toISOString().slice(0, 10)} OR ${integrityAiSettings.dailyRequests} < ${integrityAiSettings.dailyLimit})`
						]
					: []),
				or(
					isNull(integrityAiSettings.lastRequestAt),
					lt(integrityAiSettings.lastRequestAt, new Date(Date.now() - 65000))
				)
			)
		)
		.returning({ id: integrityAiSettings.orgId });
	if (!claimed.length) throw new ApiError(429, '同一组织每65秒最多请求一次，请稍后再试。');
	const key = decryptSecret(env, config.keyEnc);
	const response = (await requester(
		config.baseUrl,
		key,
		operation === 'models' ? 'models' : 'chat/completions',
		operation === 'models'
			? undefined
			: {
					model: config.model,
					stream: false,
					[config.tokenParameter]: config.maxTokens,
					messages: [
						{ role: 'system', content: operation === 'test' ? '请简短回复连接成功。' : SYSTEM },
						{ role: 'user', content: operation === 'test' ? '连接测试，不含玩家数据。' : input }
					]
				}
	)) as {
		data?: { id?: unknown }[];
		choices?: { message?: { content?: unknown }; finish_reason?: string }[];
		usage?: unknown;
	};
	if (operation === 'models')
		return {
			models: Array.isArray(response.data)
				? response.data
						.flatMap((m) => (typeof m.id === 'string' && m.id.length <= 200 ? [m.id] : []))
						.slice(0, 500)
				: []
		};
	const choice = response.choices?.[0],
		content = choice?.message?.content;
	if (typeof content !== 'string' || !content.trim() || choice?.finish_reason === 'length')
		throw new ApiError(502, '模型未返回完整文本，请检查模型或增加输出上限。');
	if (operation === 'test') return { message: '连接成功，所选模型已返回文本。' };
	let parsed: unknown;
	try {
		parsed = JSON.parse(content.replace(/^```(?:json)?\s*/, '').replace(/\s*```$/, ''));
	} catch {
		throw new ApiError(502, '模型没有返回约定的审核 JSON，请更换模型后重试。');
	}
	const validated = reviewOutput.safeParse(parsed);
	if (!validated.success) throw new ApiError(502, '模型审核结果字段不完整，未保存为有效结论。');
	const result = {
		...validated.data,
		promptVersion: PROMPT_VERSION,
		model: config.model,
		usage: response.usage ?? null,
		advisoryOnly: true
	};
	await env.db
		.insert(integrityAiReviews)
		.values({ fingerprint, caseId: bundle!.case.id, result })
		.onConflictDoNothing();
	return { result, cached: false, createdAt: new Date() };
}
