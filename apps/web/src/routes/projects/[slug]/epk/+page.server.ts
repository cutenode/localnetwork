import { loadProjectPage } from '#lib/server/project-page.ts';
import type { PageServerLoad } from './$types';

export const load: PageServerLoad = async ({ params, locals, url }) => ({
	...(await loadProjectPage(locals, params.slug)),
	// The profile's full address, printed in the kit, so a PDF still says where to find it.
	profileUrl: new URL(`/projects/${params.slug}`, url).href
});
