import { requireViewer } from '#lib/server/http.ts';
import { loadPlanner } from '#lib/server/planner-page.ts';
import type { PageServerLoad } from './$types';

/** How many inquiries and planned shows the page lists before linking to the rest. */
const SHOWN = 4;

export const load: PageServerLoad = async ({ locals, url }) => {
	const viewer = requireViewer(locals, url);

	// The threads this account started as the promoter, newest activity first, from the app's own index.
	const asked = (await locals.app.bookings.threadsFor(viewer.did)).filter(
		(thread) => thread.promoter.did === viewer.did
	);

	// Streamed: the planner comes from the account's own server, which can be slow or not answer.
	const planner = loadPlanner(locals, viewer).then((found) =>
		found.available
			? { available: true as const, shows: found.shows.slice(0, SHOWN), total: found.shows.length }
			: { available: false as const, problem: 'problem' in found ? found.problem : undefined }
	);

	return {
		geocoder: locals.app.geocoder.enabled,
		inquiries: asked.slice(0, SHOWN),
		inquiryCount: asked.length,
		planner
	};
};
