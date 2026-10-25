import { requireViewer } from '#lib/server/http.ts';
import type { PageServerLoad } from './$types';

export const load: PageServerLoad = async ({ locals, url }) => {
	const viewer = requireViewer(locals, url);
	const { app } = locals;

	const [requests, threads] = await Promise.all([
		app.requests.list({ requester: viewer.did, limit: 5 }),
		app.bookings.threadsFor(viewer.did)
	]);

	return { requests, threads: threads.slice(0, 5) };
};
