import { soon } from '#lib/server/home.ts';
import { requireViewer } from '#lib/server/http.ts';
import type { PageServerLoad } from './$types';

export const load: PageServerLoad = async ({ locals, url }) => {
	const viewer = requireViewer(locals, url);
	const records = locals.app.heldRecords.list(viewer);

	// Streamed if it's slow, so the page shows while the directory names the projects. Caught here too, so a failure only
	// fails the counts.
	records.catch((error) => locals.log.warn('could not list the records held for someone', { err: error }));

	return { records: (await soon(records)).value };
};
