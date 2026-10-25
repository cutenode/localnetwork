import { AUDIT_ACTIONS } from '#lib/server/audit/log.ts';
import { requireStaff } from '#lib/server/http.ts';
import type { Actions, PageServerLoad } from './$types';

export const load: PageServerLoad = async ({ locals, url }) => {
	requireStaff(locals, url);
	const { app } = locals;
	const actorParam = url.searchParams.get('actor')?.trim() ?? '';
	let actor: string | undefined;

	if (actorParam) actor = await app.identity.resolveIdentifier(actorParam).catch(() => actorParam);

	const filter = {
		actor,
		action: url.searchParams.get('action') ?? undefined,
		outcome: url.searchParams.get('outcome') || undefined,
		query: url.searchParams.get('q') ?? undefined,
		before: Number(url.searchParams.get('before')) || undefined,
		limit: 100
	};

	const [entries, stats, orphans] = await Promise.all([
		app.audit.list(filter),
		app.audit.stats(),
		app.audit.orphans(20)
	]);

	const accounts = await app.accounts.views(entries.map((entry) => entry.actorDid));

	return {
		entries: entries.map((entry) => ({ ...entry, actor: accounts.get(entry.actorDid)! })),
		stats,
		orphans: orphans.length,
		actionGroups: [...new Set(AUDIT_ACTIONS.map((action) => action.split('.')[0]))],
		filters: { actor: actorParam, action: filter.action ?? '', outcome: filter.outcome ?? '', q: filter.query ?? '' }
	};
};

export const actions: Actions = {
	verify: async ({ locals, url }) => {
		requireStaff(locals, url);

		return { verification: await locals.app.audit.verifyChain() };
	}
};
