import { error } from '@sveltejs/kit';
import { soon } from '#lib/server/home.ts';
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
		const { project, ...opened } = await locals.app.bookings.openThread(viewer, space);

		// The directory only adds the act's name, which streams if it's slow.
		const named = await soon(
			project.catch((cause) => {
				locals.log.warn("couldn't read a booking thread's act", { err: cause, space });

				return null;
			})
		);

		return { ...opened, project: named.value };
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

		const status = text(form, 'status', 32);

		return act(async () => {
			await locals.app.bookings.setStatus({ actor: viewer, space, status, note: optionalText(form, 'note', 5000) });

			// The pill and the conversation show the change; the page announces it by the status control.
			return { status };
		});
	}
};
