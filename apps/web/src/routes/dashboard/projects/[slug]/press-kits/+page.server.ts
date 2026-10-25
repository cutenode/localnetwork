import type { PageServerLoad } from './$types';

export const load: PageServerLoad = async ({ locals, url, parent }) => {
	const { project } = await parent();

	return {
		// Streamed, so the page shows while the managers' PDSes are asked for their press kits.
		epks: locals.app.epks.forProject(project),
		deleted: url.searchParams.has('deleted')
	};
};
