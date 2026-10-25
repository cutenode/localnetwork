import { error, redirect } from '@sveltejs/kit';
import { bookingValues, showFormDefaults } from '#lib/shows.ts';
import type { ShowFormValues } from '#lib/shows.ts';
import { parseAnnouncement, parseEdit } from '#lib/server/forms/announce.ts';
import { act, requireViewer, stringValues } from '#lib/server/http.ts';
import { requireManager } from '#lib/server/manage.ts';
import type { Viewer } from '#lib/types.ts';
import type { Actions, PageServerLoad } from './$types';

/** The show in the confirmed booking `skey` with this project, for the announce form, when the viewer is in it. */
async function fromBooking(locals: App.Locals, viewer: Viewer, projectUri: string, skey: string) {
	const space = await locals.app.bookings.bySkey(skey);
	const booking = space ? await locals.app.bookings.thread(viewer, space).catch(() => null) : null;

	if (booking?.thread.status !== 'confirmed' || booking.thread.project?.uri !== projectUri) return null;
	const show = booking.events.find((event) => event.kind === 'inquiry' && event.show)?.show;

	return show ? bookingValues(show) : null;
}

export const load: PageServerLoad = async ({ params, locals, url, parent }) => {
	const { project } = await parent();
	const viewer = requireViewer(locals, url);
	const isNew = params.rkey === 'new';
	const skey = url.searchParams.get('booking');
	let values: ShowFormValues | null = showFormDefaults;

	if (!isNew) values = await locals.app.shows.values(viewer, project, params.rkey);
	else if (skey) values = (await fromBooking(locals, viewer, project.uri, skey)) ?? showFormDefaults;

	if (!values) error(404, 'Show not found');

	return { isNew, values, geocoder: locals.app.geocoder.enabled };
};

export const actions: Actions = {
	save: async ({ params, locals, url, request }) => {
		const { viewer, project } = await requireManager(locals, url, params.slug);
		const form = await request.formData();

		const result = await act(
			async () => {
				await locals.app.shows.save(
					viewer,
					project,
					parseAnnouncement(form),
					params.rkey === 'new' ? undefined : parseEdit(form, params.rkey)
				);

				return { saved: true };
			},
			{ ...stringValues(form), cancelled: form.get('cancelled') === 'on' }
		);

		if ('saved' in result) redirect(303, `/dashboard/projects/${project.slug}#shows`);

		return result;
	},
	delete: async ({ params, locals, url }) => {
		const { viewer, project } = await requireManager(locals, url, params.slug);

		const result = await act(async () => {
			await locals.app.shows.remove(viewer, project, params.rkey);

			return { deleted: true };
		});

		if ('deleted' in result) redirect(303, `/dashboard/projects/${project.slug}#shows`);

		return result;
	}
};
