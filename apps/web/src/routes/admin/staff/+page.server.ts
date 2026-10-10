import { UserError } from '#lib/server/core.ts';
import { act, optionalText, requireAdmin, text } from '#lib/server/http.ts';
import type { Actions, PageServerLoad } from './$types';

export const load: PageServerLoad = async ({ locals, url }) => {
	requireAdmin(locals, url);

	return { staff: await locals.app.staff.list() };
};

export const actions: Actions = {
	grant: async ({ locals, url, request }) => {
		const viewer = requireAdmin(locals, url);
		const form = await request.formData();

		return act(async () => {
			const role = text(form, 'role', 16);

			if (role !== 'admin' && role !== 'moderator') throw new UserError('Pick a role');
			const did = await locals.app.identity.resolveIdentifier(text(form, 'subject', 300));
			await locals.app.staff.grant(viewer, did, role, optionalText(form, 'note', 300));

			return { success: 'Role granted' };
		});
	},
	revoke: async ({ locals, url, request }) => {
		const viewer = requireAdmin(locals, url);
		const form = await request.formData();

		return act(async () => {
			await locals.app.staff.revoke(viewer, text(form, 'did', 300));

			return { success: 'Role removed' };
		});
	}
};
