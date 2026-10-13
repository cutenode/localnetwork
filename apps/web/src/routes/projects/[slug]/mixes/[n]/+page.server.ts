import { error } from '@sveltejs/kit';
import { UserError } from '#lib/server/core.ts';
import type { PageServerLoad } from './$types';

/** A mix's tracklist (a locallibrary mix report), by the mix's position on the profile. */
export const load: PageServerLoad = async ({ params, locals, setHeaders }) => {
	const { app, viewer } = locals;
	const project = await app.projects.bySlug(params.slug);

	if (!project) error(404, 'No project with that name');
	const isManager = !!viewer && project.managers.includes(viewer.did);

	if (project.status !== 'active' && !isManager && !viewer?.isStaff) error(404, 'No project with that name');

	const n = Number(params.n);
	const mix = Number.isInteger(n) && n >= 1 ? project.profile.mixes?.[n - 1] : undefined;

	if (!mix?.tracklist) error(404, 'This mix has no tracklist');

	try {
		const report = await app.tracklists.published(mix.tracklist);

		if (project.status === 'active') setHeaders({ 'cache-control': 'public, max-age=300' });

		return {
			project: { slug: project.slug, name: project.profile.name },
			mix: { title: mix.title, url: mix.url, set: mix.set },
			// Streamed, so the report shows while the directory looks up the set.
			sets: mix.set ? app.sets.linked([mix.set]) : undefined,
			n,
			report
		};
	} catch (cause) {
		if (cause instanceof UserError) error(cause.status, cause.message);
		throw cause;
	}
};
