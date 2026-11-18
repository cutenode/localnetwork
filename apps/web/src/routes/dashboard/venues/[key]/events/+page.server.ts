import { error } from '@sveltejs/kit';
import { capabilitiesIn } from '#lib/capabilities.ts';
import { act, requireFlag, requireViewer, text } from '#lib/server/http.ts';
import type { Actions, PageServerLoad } from './$types';

/** The venue, for someone who approves events at it: people with venue.approve, and staff. */
async function approver(locals: App.Locals, url: URL, key: string) {
	const viewer = requireViewer(locals, url);

	requireFlag(locals, 'peopleSpaces');
	const venue = await locals.app.venues.bySkey(key);

	if (!venue) error(404, 'Venue not found');
	const person = (await locals.app.sharedSpaces.live(venue.space)).people.get(viewer.did);

	if (!viewer.isStaff && !capabilitiesIn('venue', person?.capabilities ?? []).includes('venue.approve'))
		error(403, 'Only people who approve events at this venue can see this');

	return { viewer, venue };
}

export const load: PageServerLoad = async ({ params, locals, url }) => {
	const { venue } = await approver(locals, url, params.key);
	const waiting = await locals.app.events.waiting(venue.space);
	const parties = await Promise.all(waiting.map((event) => locals.app.parties.bySpace(event.space)));

	return {
		venue: { name: venue.name },
		waiting: waiting.map((event, index) => ({
			uri: event.uri,
			name: event.record.name,
			// SAFETY: only a published event, which always has a start, waits for the venue.
			startsAt: event.record.startsAt!,
			party: parties[index]?.basics.name ?? null,
			lineup: event.record.lineup?.length ?? 0
		}))
	};
};

export const actions: Actions = {
	approve: async ({ request, params, locals, url }) => {
		const { viewer, venue } = await approver(locals, url, params.key);
		const form = await request.formData();

		return act(async () => {
			const uri = text(form, 'event', 2000);
			const event = await locals.app.events.get(uri);

			// Only an event naming this venue, so a form can't approve one somewhere else from here.
			if (event?.record.venue !== venue.space) error(404, 'Event not found');
			await locals.app.events.approve(viewer, uri);

			return { success: `Confirmed ${event.record.name}.` };
		});
	}
};
