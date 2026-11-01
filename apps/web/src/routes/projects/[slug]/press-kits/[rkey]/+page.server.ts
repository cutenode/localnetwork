import { error } from '@sveltejs/kit';
import { pressKitMeta } from '#lib/server/meta.ts';
import { projectRecords, withRecords } from '#lib/server/meta-records.ts';
import { loadProjectPage } from '#lib/server/project-page.ts';
import type { PageServerLoad } from './$types';

export const load: PageServerLoad = async ({ locals, params }) => {
	const page = await loadProjectPage(locals, params.slug);
	const kit = await locals.app.epks.get(page.project, params.rkey);

	if (!kit) error(404, 'Press kit not found');
	const access = await locals.app.epks.accessState(kit, locals.viewer);
	const granted = access.kind === 'granted';
	const canSee = access.kind === 'manager' || granted;

	// Someone let in sees the rest, with the project's booking details, and the project's managers get a receipt.
	if (granted && locals.viewer) await locals.app.epkViews.record(kit, locals.viewer);

	return {
		...page,
		details: granted ? ((await locals.app.projects.details(page.project.uri)) ?? {}) : page.details,
		epk: kit.epk,
		access,
		content: canSee ? await locals.app.epks.content(kit.epk) : null,
		// The project's mixes with tracklists, by URL, so the press kit can link each one's tracklist.
		tracklists: Object.fromEntries(
			(page.project.profile.mixes ?? []).flatMap((mix, index) => (mix.tracklist ? [[mix.url, index + 1]] : []))
		),
		meta: await withRecords(pressKitMeta(locals.app.config.publicUrl, page.project, kit.epk), () =>
			projectRecords(locals.app.requests, page.project, { canonical: kit.epk.uri, alternate: [page.project.uri] })
		)
	};
};
