import { requireViewer } from '#lib/server/http.ts';
import type { PageServerLoad } from './$types';

export const load: PageServerLoad = async ({ locals, url }) => {
	const viewer = requireViewer(locals, url);
	const { app } = locals;

	const [requests, threads, profile] = await Promise.all([
		app.requests.list({ requester: viewer.did, limit: 5 }),
		app.bookings.threadsFor(viewer.did),
		// Undefined while the directory can't say, so nobody is asked to set up a profile they have.
		app.profiles.get(viewer.did).catch((error) => {
			locals.log.warn({ err: error }, "couldn't tell whether the account has a profile");

			return undefined;
		})
	]);

	// Someone without a profile gets a card to make one, filled in from their Bluesky profile.
	const firstRun = profile === null ? ((await app.profiles.blueskyPrefill(viewer.did)) ?? {}) : null;

	return {
		requests,
		threads: threads.slice(0, 5),
		firstRun: firstRun && { name: firstRun.name ?? viewer.handle ?? '', hasAvatar: !!firstRun.avatar }
	};
};
