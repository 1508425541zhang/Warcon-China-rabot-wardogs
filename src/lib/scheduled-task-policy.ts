import { z } from 'zod';
export const scheduledActionSchema = z.discriminatedUnion('kind', [
	z
		.object({
			kind: z.literal('weapon_restriction'),
			enabled: z.boolean(),
			causes: z
				.array(
					z
						.string()
						.trim()
						.min(1)
						.max(200)
						.regex(/^[A-Za-z0-9_.-]+$/)
				)
				.max(1000),
			groups: z.array(z.enum(['items', 'vehicles', 'buildables'])).max(3)
		})
		.strict(),
	z.object({ kind: z.literal('faction_lock'), enabled: z.boolean() }).strict(),
	z
		.object({
			kind: z.literal('trigger'),
			triggerId: z.string().min(1).max(100),
			enabled: z.boolean()
		})
		.strict(),
	z.object({ kind: z.literal('broadcast'), message: z.string().trim().min(1).max(200) }).strict()
]);
export const scheduleInputSchema = z
	.object({
		name: z.string().trim().min(1).max(80),
		when: z.discriminatedUnion('kind', [
			z.object({ kind: z.literal('at'), at: z.string().datetime({ offset: true }) }).strict(),
			z.object({ kind: z.literal('next_match') }).strict()
		]),
		action: scheduledActionSchema
	})
	.strict()
	.refine(
		(v) =>
			v.action.kind !== 'weapon_restriction' ||
			!v.action.enabled ||
			v.action.causes.length + v.action.groups.length > 0,
		'启用武器限制至少需要选择一种来源。'
	);
export type ScheduledAction = z.infer<typeof scheduledActionSchema>;
export type ScheduleInput = z.infer<typeof scheduleInputSchema>;
export type ScheduledTask = ScheduleInput & {
	id: string;
	authorId: string;
	createdAt: string;
	baselineMatch: number;
	state: 'pending' | 'applied' | 'queued' | 'cancelled' | 'skipped';
	finishedAt: string | null;
	outcome: string;
};
export type ScheduleStore = { revision: string; enabled: boolean; tasks: ScheduledTask[] };
export const SCHEDULE_GRACE_MS = 5 * 60 * 1000;
export function scheduleDue(
	task: ScheduledTask,
	now: number,
	match: { id: number; startedAt: Date } | null
) {
	if (task.state !== 'pending') return 'wait';
	const due =
		task.when.kind === 'at'
			? Date.parse(task.when.at)
			: match && match.id > task.baselineMatch
				? Math.max(match.startedAt.getTime(), Date.parse(task.createdAt))
				: null;
	if (due === null || now < due) return 'wait';
	return now - due > SCHEDULE_GRACE_MS ? 'expired' : 'run';
}
export const scheduleActionLabel = (a: ScheduledAction) =>
	a.kind === 'weapon_restriction'
		? `${a.enabled ? '启用' : '关闭'}武器限制`
		: a.kind === 'faction_lock'
			? `${a.enabled ? '禁止' : '允许'}玩家自行换阵营`
			: a.kind === 'trigger'
				? `${a.enabled ? '启用' : '关闭'}自动化规则`
				: '服务器广播';
