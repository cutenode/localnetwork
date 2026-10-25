import { redirect } from '@sveltejs/kit';
import { saveEpkForm } from '#lib/server/epk-page.ts';
import { epkFormValues, submittedValues } from '#lib/server/forms/epk.ts';
import { act, requireViewer } from '#lib/server/http.ts';
import { requireManager } from '#lib/server/manage.ts';
import type { Actions, PageServerLoad } from './$types';

export const load: PageServerLoad = async ({ locals, url, parent }) => {
	const { project } = await parent();
	const viewer = requireViewer(locals, url);

	return { values: epkFormValues(project.profile.mixes ?? []), spacesCapable: viewer.spacesCapable };
};

export const actions: Actions = {
	default: async ({ request, locals, url, params }) => {
		const { viewer, project } = await requireManager(locals, url, params.slug);
		const form = await request.formData();

		const result = await act(
			() => saveEpkForm(locals.app.epks, viewer, project, form, []),
			submittedValues(form, project.profile.mixes ?? [])
		);

		if ('rkey' in result)
			redirect(303, `/dashboard/projects/${project.slug}/press-kits/${result.did}/${result.rkey}?saved=1`);

		return result;
	}
};
