import { loadProjectPage } from '#lib/server/project-page.ts';
import type { PageServerLoad } from './$types';

export const load: PageServerLoad = async ({ params, locals, url }) => {
	const page = await loadProjectPage(locals, params.slug);

	return {
		...page,
		// Streamed, so a slow read of the root's press kits never holds the page up.
		epks: locals.app.epks.forProject(page.project),
		// The profile's full address, printed in the kit, so a PDF still says where to find it.
		profileUrl: new URL(`/projects/${params.slug}`, url).href
	};
};
