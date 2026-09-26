import { and, count, desc, eq, inArray, lte } from 'drizzle-orm';
import { error } from '@sveltejs/kit';
import type { PageServerLoad } from './$types';
import { getEnv } from '$lib/server/env';
import { requireServerCap, orgRoleFor } from '$lib/server/access';
import { normalizeError } from '$lib/server/http';
import {
	integrityCases,
	steamProfiles,
	integrityLabels,
	integrityActions,
	listEntries
} from '$lib/server/db/schema';
import { aiJobViews } from '$lib/server/integrity/ai-queue';
import { aiSettings } from '$lib/server/integrity/ai';

export const load: PageServerLoad = async ({ locals, params, url }) => {
	const env = getEnv();
	try {
		const { server, user } = await requireServerCap(env, locals, params.id, 'integrity.view');
		const now = new Date(),
			raw = url.searchParams.get('before');
		const parsed = raw ? Date.parse(raw) : NaN;
		const before = Number.isFinite(parsed) && parsed <= now.getTime() ? new Date(parsed) : now;
		const scope = and(
			eq(integrityCases.serverId, server.id),
			lte(integrityCases.createdAt, before)
		);
		const [totalRow] = await env.db.select({ total: count() }).from(integrityCases).where(scope);
		const total = Number(totalRow.total),
			pageSize = 20,
			pages = Math.max(1, Math.ceil(total / pageSize));
		const requested = Number(url.searchParams.get('page') ?? 1);
		const page = Number.isSafeInteger(requested) ? Math.min(pages, Math.max(1, requested)) : 1;
		const cases = await env.db
			.select({
				name: steamProfiles.persona,
				id: integrityCases.id,
				steamId: integrityCases.steamId,
				createdAt: integrityCases.createdAt,
				status: integrityCases.status,
				confidence: integrityCases.confidence,
				riskScore: integrityCases.riskScore,
				riskBreakdown: integrityCases.riskBreakdown,
				statistical: integrityCases.statistical,
				snapshot: integrityCases.snapshot,
				trigger: integrityCases.trigger
			})
			.from(integrityCases)
			.leftJoin(steamProfiles, eq(steamProfiles.steamId, integrityCases.steamId))
			.where(scope)
			.orderBy(desc(integrityCases.createdAt), desc(integrityCases.id))
			.limit(pageSize)
			.offset((page - 1) * pageSize);
		const ids = cases.map((c) => c.id);
		const [labels, penalties, aiJobs, settings, role] = await Promise.all([
			ids.length
				? env.db
						.select()
						.from(integrityLabels)
						.where(inArray(integrityLabels.caseId, ids))
						.orderBy(desc(integrityLabels.createdAt))
				: [],
			ids.length
				? env.db
						.select({ action: integrityActions, entry: listEntries })
						.from(integrityActions)
						.leftJoin(listEntries, eq(listEntries.id, integrityActions.listEntryId))
						.where(
							and(
								eq(integrityActions.serverId, server.id),
								eq(integrityActions.source, 'REVIEW'),
								inArray(integrityActions.caseId, ids)
							)
						)
				: [],
			aiJobViews(env, ids),
			aiSettings(env, server.orgId),
			orgRoleFor(env, user, server.orgId)
		]);
		return {
			page,
			pages,
			pageSize,
			total,
			before: before.toISOString(),
			cases: cases.map((c) => ({ ...c, createdAt: c.createdAt.toISOString() })),
			labels: labels.map((l) => ({
				caseId: l.caseId,
				label: l.label,
				reason: l.reason,
				createdAt: l.createdAt.toISOString()
			})),
			aiJobs,
			aiAutoEnabled: !!settings?.autoEnabled,
			canConfigure: role === 'owner',
			reviewPenalties: penalties.map(({ action, entry }) => ({
				caseId: action.caseId,
				id: action.id,
				deliveryState: action.deliveryState,
				expiresAt: entry?.expiresAt?.toISOString() ?? null,
				active:
					!!entry &&
					!entry.removedAt &&
					!action.revertedAt &&
					(!entry.expiresAt || entry.expiresAt > now)
			}))
		};
	} catch (err) {
		const known = normalizeError(err);
		if (!known) throw err;
		error(known.status, known.message);
	}
};
