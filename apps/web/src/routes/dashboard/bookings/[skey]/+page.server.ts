import { error } from '@sveltejs/kit';
import { act, optionalText, requireViewer, text } from '#lib/server/http.ts';
import type { Actions, PageServerLoad } from './$types';

async function spaceFor(locals: App.Locals, skey: string) {
	const space = await locals.app.bookings.bySkey(skey);

	if (!space) error(404, 'Booking thread not found');

	return space;
}

export const load: PageServerLoad = async ({ params, locals, url }) => {
	const viewer = requireViewer(locals, url);
	const space = await spaceFor(locals, params.skey);

	try {
		return await locals.app.bookings.thread(viewer, space);
	} catch {
		error(404, 'Booking thread not found');
	}
};

export const actions: Actions = {
	message: async ({ params, locals, url, request }) => {
		const viewer = requireViewer(locals, url);
		const space = await spaceFor(locals, params.skey);
		const form = await request.formData();

		return act(async () => {
			await locals.app.bookings.message({ actor: viewer, space, text: text(form, 'text', 20_000) });

			return { sent: true };
		});
	},
	status: async ({ params, locals, url, request }) => {
		const viewer = requireViewer(locals, url);
		const space = await spaceFor(locals, params.skey);
		const form = await request.formData();

		return act(async () => {
			await locals.app.bookings.setStatus({
				actor: viewer,
				space,
				status: text(form, 'status', 32),
				note: optionalText(form, 'note', 5000)
			});

			return { success: 'Status updated' };
		});
	}
};
