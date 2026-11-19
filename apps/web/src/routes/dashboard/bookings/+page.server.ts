import { soon } from '#lib/server/home.ts';
import { requireViewer } from '#lib/server/http.ts';
import type { PageServerLoad } from './$types';

export const load: PageServerLoad = async ({ locals, url }) => {
	const viewer = requireViewer(locals, url);
	const { threads, acts, projects } = await locals.app.bookings.listFor(viewer.did);

	// The threads are the site's own and show at once; the directory only adds act names, which stream if it's slow.
	const names = await soon(
		projects.catch((error) => {
			locals.log.warn("couldn't read the acts of booking threads", { err: error });

			return null;
		})
	);

	return { threads, acts, names: names.value };
};
