import { error } from '@sveltejs/kit';
import { nextNights, soon } from '#lib/server/home.ts';
import { requireViewer } from '#lib/server/http.ts';
import type { PageServerLoad } from './$types';

/** A year ahead is as far as anyone books; further out is a plan, not a night to keep an eye on. */
const DAYS_AHEAD = 365;

export const load: PageServerLoad = async ({ locals, url, parent }) => {
	// Behind a release flag while it's tried against real bookings; off, the page isn't there.
	if (!locals.flags.dashboardCalendar) error(404, 'Not found');

	const viewer = requireViewer(locals, url);
	const { projects, sections } = await parent();

	// Home's next nights, all of them: the directory's shows can be slow, so this streams when they are.
	const nights = locals.app.bookings
		.threadsFor(viewer.did)
		.then(async (threads) =>
			nextNights(locals, viewer, threads, await projects, await sections, { shown: Infinity, ahead: DAYS_AHEAD })
		);

	const { value } = await soon(nights);

	return { nights: value };
};
