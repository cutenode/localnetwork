import { error } from '@sveltejs/kit';
import { flagInput, requireGrant } from '#lib/server/forms/flags.ts';
import { act } from '#lib/server/http.ts';
import type { Actions, PageServerLoad } from './$types';

export const load: PageServerLoad = async ({ locals, url, params }) => {
	await requireGrant(locals, url, 'flags');
	const flag = await locals.app.flags.flag(params.key);

	if (!flag) error(404, 'There is no flag with that key');

	return { flag, groups: await locals.app.flags.groups() };
};

export const actions: Actions = {
	save: async ({ locals, url, params, request }) => {
		const viewer = await requireGrant(locals, url, 'flags');
		const form = await request.formData();

		return act(async () => {
			await locals.app.flags.updateFlag(viewer, params.key, await flagInput(form, locals.app.identity));

			return { success: 'Flag saved' };
		});
	}
};
