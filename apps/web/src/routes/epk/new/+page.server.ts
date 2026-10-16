import { error, redirect } from '@sveltejs/kit';
import { saveEpkForm } from '#lib/server/epk-page.ts';
import { epkFormValues, submittedValues } from '#lib/server/forms/epk.ts';
import { act, requireViewer } from '#lib/server/http.ts';
import type { Actions, PageServerLoad } from './$types';

async function managedProject(locals: App.Locals, url: URL) {
	const viewer = requireViewer(locals, url);
	const project = await locals.app.projects.bySlug(url.searchParams.get('project') ?? '');

	if (!project) error(404, 'Pick a project to make a press kit for');

	if (!project.managers.includes(viewer.did)) error(403, 'Only managers of this project can publish press kits for it');

	return { viewer, project };
}

export const load: PageServerLoad = async ({ locals, url }) => {
	const { viewer, project } = await managedProject(locals, url);

	return { project, values: epkFormValues(project.profile.mixes ?? []), spacesCapable: viewer.spacesCapable };
};

export const actions: Actions = {
	default: async ({ request, locals, url }) => {
		const { viewer, project } = await managedProject(locals, url);
		const form = await request.formData();

		const result = await act(
			() => saveEpkForm(locals.app.epks, viewer, project, form, []),
			submittedValues(form, project.profile.mixes ?? [])
		);

		if ('rkey' in result) redirect(303, `/epk/${result.did}/${result.rkey}?saved=1`);

		return result;
	}
};
