import { redirect } from '@sveltejs/kit';
import { flagGroupInput, requireGrant } from '#lib/server/forms/flags.ts';
import { act } from '#lib/server/http.ts';
import type { Actions, PageServerLoad } from './$types';

export const load: PageServerLoad = async ({ locals, url }) => {
	await requireGrant(locals, url, 'flagGroups');

	return { groups: await locals.app.flags.groups() };
};

export const actions: Actions = {
	create: async ({ locals, url, request }) => {
		const viewer = await requireGrant(locals, url, 'flagGroups');
		const form = await request.formData();

		return act(async () => {
			const rkey = await locals.app.flags.createGroup(viewer, await flagGroupInput(form, locals.app.identity));
			redirect(303, `/admin/flags/groups/${rkey}`);
		});
	}
};
