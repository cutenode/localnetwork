import { redirect } from '@sveltejs/kit';
import { loadEpk } from '#lib/server/epk-page.ts';
import { act, requireViewer, text } from '#lib/server/http.ts';
import type { Actions, PageServerLoad } from './$types';

export const load: PageServerLoad = async ({ locals, params, url }) => {
	const viewer = requireViewer(locals, url);
	const { epk, project } = await loadEpk(locals.app.epks, params.did, params.rkey);
	const access = await locals.app.epks.accessState(epk, viewer);

	// The one who made it decides who gets in, which happens on its own dashboard page.
	if (access.kind === 'owner')
		redirect(303, `/dashboard/projects/${project.slug}/press-kits/${epk.did}/${epk.rkey}/access`);

	return { epk, project, access };
};

export const actions: Actions = {
	request: async ({ request, locals, params, url }) => {
		const viewer = requireViewer(locals, url);
		const { epk } = await loadEpk(locals.app.epks, params.did, params.rkey);
		const form = await request.formData();

		const result = await act(async () => {
			const state = await locals.app.epks.request(viewer, epk, {
				role: text(form, 'role', 32),
				message: text(form, 'message', 1000)
			});

			return { requested: state.kind };
		});

		// Let in at once by the press kit's rules, so straight back to all of it.
		if ('requested' in result && result.requested === 'granted') redirect(303, `/epk/${epk.did}/${epk.rkey}`);

		return result;
	}
};
