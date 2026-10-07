import { requireViewer } from '#lib/server/http.ts';
import type { PageServerLoad } from './$types';

export const load: PageServerLoad = async ({ locals, url }) => {
	const viewer = requireViewer(locals, url);
	const { app } = locals;

	const [projects, requests, threads, account] = await Promise.all([
		app.projects.managedBy(viewer.did),
		app.requests.list({ requester: viewer.did, limit: 50 }),
		app.bookings.threadsFor(viewer.did),
		app.accounts.get(viewer.did)
	]);

	return { projects, requests, threads: threads.slice(0, 5), account };
};
