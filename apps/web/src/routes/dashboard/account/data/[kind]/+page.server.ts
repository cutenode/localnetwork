import { error } from '@sveltejs/kit';
import { pluralize } from '#lib/format.ts';
import { act, requireViewer, text } from '#lib/server/http.ts';
import { isHeldRecordKind } from '#lib/server/services/held-records.ts';
import type { Actions, PageServerLoad } from './$types';

export const load: PageServerLoad = async ({ locals, url, params }) => {
	const viewer = requireViewer(locals, url);
	const { kind } = params;

	if (!isHeldRecordKind(kind)) error(404, 'Not found');

	const records = locals.app.heldRecords.list(viewer).then((all) => all.filter((record) => record.kind === kind));

	// Streamed, so the page shows while the directory names the projects. Caught here too, so a failure only fails the list.
	records.catch((failure) => locals.log.warn('could not list the records held for someone', { err: failure }));

	return { kind, records };
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
