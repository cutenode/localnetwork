import { requireStaff } from '#lib/server/http.ts';
import type { PageServerLoad } from './$types';

export const load: PageServerLoad = async ({ locals, url }) => {
	requireStaff(locals, url);

	return {
		// Streamed, so the page shows at once and the list fills in.
		spaces: locals.app.sharedSpaces.list().catch((error) => {
			locals.log.warn({ err: error }, 'could not list shared spaces');

			return null;
		})
	};
};
