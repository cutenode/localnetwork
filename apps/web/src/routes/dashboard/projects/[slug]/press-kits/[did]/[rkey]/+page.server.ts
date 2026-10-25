import { redirect } from '@sveltejs/kit';
import { ownedEpk, saveEpkForm } from '#lib/server/epk-page.ts';
import { epkFormValues, submittedValues } from '#lib/server/forms/epk.ts';
import { act } from '#lib/server/http.ts';
import type { Actions, PageServerLoad } from './$types';

export const load: PageServerLoad = async ({ locals, url, params }) => {
	const { viewer, epk, project } = await ownedEpk(locals, url, params);

	// Requests the rules approved while the owner was away get their grants first, so they don't count as waiting.
	await locals.app.epks.settle(viewer);
	const [content, requests] = await Promise.all([locals.app.epks.content(epk), locals.app.epks.requests(epk)]);

	return {
		epk,
		values: epkFormValues(project.profile.mixes ?? [], epk, content),
		spacesCapable: viewer.spacesCapable,
		waiting: requests.filter((request) => request.status === 'pending').length,
		saved: url.searchParams.has('saved')
	};
};

export const actions: Actions = {
	save: async ({ request, locals, url, params }) => {
		const { viewer, epk, project } = await ownedEpk(locals, url, params);
		const form = await request.formData();
		const content = await locals.app.epks.content(epk);
		const photos = [...(epk.record.photos ?? []), ...(content?.photos ?? [])];

		const result = await act(
			() => saveEpkForm(locals.app.epks, viewer, project, form, photos, epk.rkey),
			submittedValues(form, project.profile.mixes ?? [])
		);

		if ('rkey' in result)
			redirect(303, `/dashboard/projects/${project.slug}/press-kits/${result.did}/${result.rkey}?saved=1`);

		return result;
	},
	delete: async ({ locals, url, params }) => {
		const { viewer, epk, project } = await ownedEpk(locals, url, params);
		const result = await act(() => locals.app.epks.remove(viewer, epk));

		if (result === undefined) redirect(303, `/dashboard/projects/${project.slug}/press-kits?deleted=1`);

		return result;
	}
};
