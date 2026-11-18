import { error } from '@sveltejs/kit';
import { capabilitiesIn } from '#lib/capabilities.ts';
import { UserError } from '#lib/server/core.ts';
import { act, requireFlag, requireViewer, text } from '#lib/server/http.ts';
import { blockSubject, blockViews } from '#lib/server/venue-blocks.ts';
import { BLOCK_KINDS, isOneOf, labelFor, VENUE_APPROVALS } from '#lib/taxonomy/options.ts';
import type { Actions, PageServerLoad } from './$types';

/** The venue, and what the viewer can change on it: its approval mode with page.edit, its blocks with venue.block. */
async function settings(locals: App.Locals, url: URL, key: string) {
	const viewer = requireViewer(locals, url);

	requireFlag(locals, 'peopleSpaces');
	const { app } = locals;
	const venue = await app.venues.bySkey(key);

	if (!venue) error(404, 'Venue not found');
	const person = (await app.sharedSpaces.live(venue.space)).people.get(viewer.did);
	const held = person ? capabilitiesIn('venue', person.capabilities) : [];

	const can = {
		edit: viewer.isStaff || held.includes('page.edit'),
		block: viewer.isStaff || held.includes('venue.block')
	};

	if (!can.edit && !can.block) error(403, "Only the venue's owners can change its settings");

	return { viewer, venue, can };
}

export const load: PageServerLoad = async ({ params, locals, url }) => {
	const { venue, can } = await settings(locals, url, params.key);

	return {
		venue: { name: venue.name, approval: venue.approval },
		can,
		blocks: can.block ? await blockViews(locals.app, await locals.app.venues.blocks(venue.space)) : []
	};
};

export const actions: Actions = {
	approval: async ({ request, params, locals, url }) => {
		const { viewer, venue } = await settings(locals, url, params.key);
		const form = await request.formData();

		return act(async () => {
			const approval = text(form, 'approval', 16);

			if (!isOneOf(VENUE_APPROVALS, approval)) throw new UserError('Choose how events show');
			await locals.app.venues.setApproval(viewer, venue.space, approval);
			// Which of its events are listed changes with how events show, and their calendar copies with them.
			await locals.app.events.reconcileCalendarCopies(venue.space);

			return { success: `Saved: ${labelFor(VENUE_APPROVALS, approval)}.` };
		});
	},

	block: async ({ request, params, locals, url }) => {
		const { viewer, venue } = await settings(locals, url, params.key);
		const form = await request.formData();
		const values = { kind: text(form, 'kind', 16), name: text(form, 'name', 300) };

		return act(async () => {
			if (!isOneOf(BLOCK_KINDS, values.kind)) throw new UserError('Choose who to block');
			await locals.app.venues.block(viewer, venue.space, await blockSubject(locals.app, values.kind, values.name));
			// A block keeps the venue from confirming an event, which can take it off the list, and its calendar copy.
			await locals.app.events.reconcileCalendarCopies(venue.space);

			return { success: 'Blocked.' };
		}, values);
	},

	unblock: async ({ request, params, locals, url }) => {
		const { viewer, venue } = await settings(locals, url, params.key);
		const form = await request.formData();

		return act(async () => {
			await locals.app.venues.unblock(viewer, venue.space, text(form, 'rkey', 32));
			await locals.app.events.reconcileCalendarCopies(venue.space);

			return { success: 'Block taken away.' };
		});
	}
};
