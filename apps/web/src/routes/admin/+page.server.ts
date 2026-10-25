import { requireStaff } from '#lib/server/http.ts';
import type { PageServerLoad } from './$types';

export const load: PageServerLoad = async ({ locals, url }) => {
	requireStaff(locals, url);
	const { app } = locals;

	const [requests, projects, audit, recent, sync, pending] = await Promise.all([
		app.requests.counts(),
		app.projects.stats(),
		app.audit.stats(),
		app.audit.list({ limit: 8 }),
		app.syncer.status(),
		app.requests.list({ status: 'pending', limit: 5 })
	]);

	const accounts = await app.accounts.views(recent.map((entry) => entry.actorDid));

	return {
		requests,
		projects,
		audit,
		recent: recent.map((entry) => ({ ...entry, actor: accounts.get(entry.actorDid)! })),
		syncErrors: sync.filter((space) => space.lastError).length,
		spaces: sync.length,
		pending
	};
};
