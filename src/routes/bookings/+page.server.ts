import { requireViewer } from '#lib/server/http.ts';
import type { PageServerLoad } from './$types';

export const load: PageServerLoad = async ({ locals, url }) => {
	const viewer = requireViewer(locals, url);

	return { threads: await locals.app.bookings.threadsFor(viewer.did) };
};
