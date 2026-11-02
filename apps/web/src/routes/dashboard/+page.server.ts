import { bookingGroup } from '#lib/bookings.ts';
import { requireViewer } from '#lib/server/http.ts';
import { loadPlanner } from '#lib/server/planner-page.ts';
import type { PageServerLoad } from './$types';

export const load: PageServerLoad = async ({ locals, url, parent }) => {
	const viewer = requireViewer(locals, url);
	const { app } = locals;
	const { projects, sections } = await parent();

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

	// Threads waiting on the viewer come first; the rest stay newest first.
	const waiting = threads.filter((thread) => bookingGroup(thread, viewer.did) === 'reply');

	// Someone who manages no projects, on a profile that isn't a DJ's, books DJs, so their overview leads with the
	// planner. Streamed, after the projects, so a DJ's overview never opens their planner.
	const books = Promise.all([projects, sections]).then(
		([managed, shown]) => managed?.length === 0 && !shown?.performer
	);

	return {
		requests,
		threads: [...waiting, ...threads.filter((thread) => !waiting.includes(thread))].slice(0, 5),
		waiting: waiting.length,
		firstRun: firstRun && { name: firstRun.name ?? viewer.handle ?? '', hasAvatar: !!firstRun.avatar },
		books,
		planner: books.then((booker) => (booker ? loadPlanner(locals, viewer) : null))
	};
};
