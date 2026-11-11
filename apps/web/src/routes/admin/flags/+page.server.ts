import { requireGrant } from '#lib/server/forms/flags.ts';
import { PREVIEW_COOKIE } from '#lib/server/flags/preview.ts';
import type { Actions, PageServerLoad } from './$types';

export const load: PageServerLoad = async ({ locals, url }) => {
	await requireGrant(locals, url, 'flags');

	return { flags: await locals.app.flags.list() };
};

export const actions: Actions = {
	/** The banner's Clear, from any page: every flag back to what everyone else sees, in this browser. */
	clearPreview: async ({ locals, url, cookies }) => {
		const viewer = await requireGrant(locals, url, 'flags');

		for (const key of Object.keys(locals.flagPreview ?? {})) await locals.app.flags.recordPreview(viewer, key, null);
		cookies.delete(PREVIEW_COOKIE, { path: '/' });

		return { success: 'Every flag is back to what everyone else sees' };
	}
};
