import { error, redirect } from '@sveltejs/kit';
import { parseClaim } from '#lib/server/forms/claim.ts';
import { act, optionalText, requireViewer, stringValues } from '#lib/server/http.ts';
import type { Actions, PageServerLoad } from './$types';

export const load: PageServerLoad = async ({ params, locals, url }) => {
	const viewer = requireViewer(locals, url);
	const project = await locals.app.projects.bySlug(params.slug);

	if (!project || project.status === 'archived') error(404, 'Project not found');

	if (project.managers.includes(viewer.did)) redirect(303, `/projects/${project.slug}`);

	return { project };
};

export const actions: Actions = {
	default: async ({ request, params, locals, url }) => {
		const viewer = requireViewer(locals, url);
		const project = await locals.app.projects.bySlug(params.slug);

		if (!project) error(404, 'Project not found');
		const form = await request.formData();

		const result = await act(async () => {
			const submitted = await locals.app.requests.submit({
				actor: viewer,
				kind: 'claim',
				projectUri: project.uri,
				claim: parseClaim(form),
				message: optionalText(form, 'message', 20_000)
			});

			return { rkey: submitted.rkey };
		}, stringValues(form));

		if ('rkey' in result) redirect(303, `/me/requests/${result.rkey}?submitted=1`);

		return result;
	}
};
