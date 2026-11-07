import { act, requireViewer, text } from '#lib/server/http.ts';
import { isHeldRecordKind } from '#lib/server/services/held-records.ts';
import { pluralize } from '#lib/format.ts';
import type { Actions, PageServerLoad } from './$types';

export const load: PageServerLoad = async ({ locals, url }) => {
	const viewer = requireViewer(locals, url);
	const records = locals.app.heldRecords.list(viewer);

	// Streamed, so the page shows while the directory names the projects. Caught here too, so a failure only fails the list.
	records.catch((error) => locals.log.warn('could not list the records held for someone', { err: error }));

	return { records };
};

export const actions: Actions = {
	delete: async ({ locals, url, request }) => {
		const viewer = requireViewer(locals, url);
		const uri = text(await request.formData(), 'uri', 2_000);

		return act(async () => {
			await locals.app.heldRecords.delete(viewer, uri);

			return { success: 'Deleted.' };
		});
	},
	deleteAll: async ({ locals, url, request }) => {
		const viewer = requireViewer(locals, url);
		const kind = text(await request.formData(), 'kind', 32);

		return act(async () => {
			if (!isHeldRecordKind(kind)) return { success: 'Nothing to delete.' };
			const count = await locals.app.heldRecords.deleteAll(viewer, kind);

			return { success: count ? `Deleted ${pluralize(count, 'record')}.` : 'Nothing to delete.' };
		});
	}
};
