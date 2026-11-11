import { error } from '@sveltejs/kit';
import { UserError } from '#lib/server/core.ts';
import { flagInput, requireGrant, writePreview } from '#lib/server/forms/flags.ts';
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
	},
	preview: async ({ locals, url, params, request, cookies }) => {
		const viewer = await requireGrant(locals, url, 'flags');
		const state = (await request.formData()).get('state');

		return act(async () => {
			if (state !== 'on' && state !== 'off' && state !== 'clear') throw new UserError('Pick on, off or clear');
			const on = state === 'clear' ? null : state === 'on';
			await locals.app.flags.recordPreview(viewer, params.key, on);
			const others = Object.entries(locals.flagPreview ?? {}).filter(([key]) => key !== params.key);
			writePreview(cookies, locals, viewer, Object.fromEntries(on === null ? others : [...others, [params.key, on]]));

			return {
				success:
					on === null
						? `${params.key} is back to what everyone else sees`
						: `${params.key} is ${state} in this browser only`
			};
		});
	}
};
