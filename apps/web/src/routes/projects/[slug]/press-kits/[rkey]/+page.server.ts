import { error } from '@sveltejs/kit';
import { loadProjectPage } from '#lib/server/project-page.ts';
import type { PageServerLoad } from './$types';

export const load: PageServerLoad = async ({ locals, params }) => {
	const page = await loadProjectPage(locals, params.slug);
	const kit = await locals.app.epks.get(page.project, params.rkey);

	if (!kit) error(404, 'Press kit not found');
	const access = await locals.app.epks.accessState(kit, locals.viewer);
	const canSee = access.kind === 'manager' || access.kind === 'granted';

	return {
		...page,
		epk: kit.epk,
		access,
		content: canSee ? await locals.app.epks.content(kit.epk) : null,
		// The project's mixes with tracklists, by URL, so the press kit can link each one's tracklist.
		tracklists: Object.fromEntries(
			(page.project.profile.mixes ?? []).flatMap((mix, index) => (mix.tracklist ? [[mix.url, index + 1]] : []))
		)
	};
};
