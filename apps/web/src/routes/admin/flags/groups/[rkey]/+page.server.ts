import { error, redirect } from '@sveltejs/kit';
import { flagGroupInput, requireGrant } from '#lib/server/forms/flags.ts';
import { act } from '#lib/server/http.ts';
import type { Actions, PageServerLoad } from './$types';

export const load: PageServerLoad = async ({ locals, url, params }) => {
	await requireGrant(locals, url, 'flagGroups');
	const group = await locals.app.flags.group(params.rkey);

	if (!group) error(404, 'There is no group with that key');

	return { group };
};

export const actions: Actions = {
	save: async ({ locals, url, params, request }) => {
		const viewer = await requireGrant(locals, url, 'flagGroups');
		const form = await request.formData();

		return act(async () => {
			await locals.app.flags.updateGroup(viewer, params.rkey, await flagGroupInput(form, locals.app.identity));

			return { success: 'Group saved' };
		});
	},
	delete: async ({ locals, url, params }) => {
		const viewer = await requireGrant(locals, url, 'flagGroups');

		return act(async () => {
			await locals.app.flags.deleteGroup(viewer, params.rkey);
			redirect(303, '/admin/flags/groups');
		});
	}
};
