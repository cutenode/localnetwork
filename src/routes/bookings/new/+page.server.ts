import { error, redirect } from '@sveltejs/kit';
import { parseShow } from '#lib/server/forms/show.ts';
import { act, requireViewer, stringValues, text } from '#lib/server/http.ts';
import type { Actions, PageServerLoad } from './$types';

export const load: PageServerLoad = async ({ locals, url }) => {
	const viewer = requireViewer(locals, url);
	const project = await locals.app.projects.bySlug(url.searchParams.get('project') ?? '');

	if (!project || project.status !== 'active') error(404, 'Pick a project to contact first');

	if (project.managers.includes(viewer.did)) redirect(303, `/projects/${project.slug}`);

	return { project, date: url.searchParams.get('date') ?? '' };
};

export const actions: Actions = {
	default: async ({ request, locals, url }) => {
		const viewer = requireViewer(locals, url);
		const project = await locals.app.projects.bySlug(url.searchParams.get('project') ?? '');

		if (!project) error(404, 'Project not found');
		const form = await request.formData();

		const result = await act(async () => {
			const space = await locals.app.bookings.start({
				actor: viewer,
				projectUri: project.uri,
				show: parseShow(form),
				message: text(form, 'message', 20_000)
			});

			return { skey: space.split('/').at(-1)! };
		}, stringValues(form));

		if ('skey' in result) redirect(303, `/bookings/${result.skey}`);

		return result;
	}
};
