import { error, redirect } from '@sveltejs/kit';
import { loadEpk, saveEpkForm } from '#lib/server/epk-page.ts';
import { epkFormValues, submittedValues } from '#lib/server/forms/epk.ts';
import { act, requireViewer } from '#lib/server/http.ts';
import type { Actions, PageServerLoad } from './$types';

async function owned(locals: App.Locals, url: URL, did: string, rkey: string) {
	const viewer = requireViewer(locals, url);

	if (viewer.did !== did) error(403, 'Only the owner of this press kit can edit it');

	return { viewer, ...(await loadEpk(locals.app.epks, did, rkey)) };
}

export const load: PageServerLoad = async ({ locals, url, params }) => {
	const { viewer, epk, project } = await owned(locals, url, params.did, params.rkey);
	const content = await locals.app.epks.content(epk);

	return {
		epk,
		project,
		values: epkFormValues(project.profile.mixes ?? [], epk, content),
		spacesCapable: viewer.spacesCapable
	};
};

export const actions: Actions = {
	save: async ({ request, locals, url, params }) => {
		const { viewer, epk, project } = await owned(locals, url, params.did, params.rkey);
		const form = await request.formData();
		const content = await locals.app.epks.content(epk);
		const photos = [...(epk.record.photos ?? []), ...(content?.photos ?? [])];

		const result = await act(
			() => saveEpkForm(locals.app.epks, viewer, project, form, photos, epk.rkey),
			submittedValues(form, project.profile.mixes ?? [])
		);

		if ('rkey' in result) redirect(303, `/epk/${result.did}/${result.rkey}?saved=1`);

		return result;
	},
	delete: async ({ locals, url, params }) => {
		const { viewer, epk, project } = await owned(locals, url, params.did, params.rkey);
		const result = await act(() => locals.app.epks.remove(viewer, epk));

		if (result === undefined) redirect(303, `/projects/${project.slug}/epk?deleted=1`);

		return result;
	}
};
