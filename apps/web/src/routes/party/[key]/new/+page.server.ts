import { error, redirect } from '@sveltejs/kit';
import type { PageContext } from '#lib/components/edit/types.ts';
import { parseSpaceRecordUri } from '#lib/server/atproto/spaces.ts';
import { TIME_ZONES } from '#lib/server/event-page.ts';
import { parseEventForm } from '#lib/server/forms/event.ts';
import { act, requireFlag, requireViewer, stringValues } from '#lib/server/http.ts';
import { pageMeta } from '#lib/server/meta.ts';
import { findParty } from '#lib/server/party-page.ts';
import { eventPagePath, spacePagePath } from '#lib/space-paths.ts';
import type { Actions, PageServerLoad, RequestEvent } from './$types';

/** The party the event is for, which the viewer must be one of the people with events.manage in. */
async function partyFor({ locals, params, url }: RequestEvent) {
	requireFlag(locals, 'spacePages');
	const viewer = requireViewer(locals, url);
	const { party, can } = await findParty(locals, params.key);

	if (!can.includes('events.manage')) error(403, "Only the party's hosts can add its events");

	return { viewer, party };
}

/**
 * An event being made: its page, empty and in edit mode, as a draft of the party's, on the time zone a new draft
 * starts on until it has a venue.
 */
export const load: PageServerLoad = async (event) => {
	const { viewer, party } = await partyFor(event);
	const { name } = party.basics;

	const context: PageContext = {
		subject: { kind: 'event', uri: 'event:new' },
		can: ['page.edit', 'events.manage'],
		role: viewer.isStaff ? 'owner-staff' : 'owner'
	};

	return {
		...context,
		party: { name, href: spacePagePath('party', party.skey, name) },
		timeZone: await event.locals.app.events.homeZone(party.space, viewer.did),
		timeZones: TIME_ZONES,
		meta: pageMeta(event.locals.app.config.publicUrl, {
			title: 'New event',
			description: `Add an event to ${name}.`,
			path: `/party/${party.skey}/new`,
			noindex: true
		})
	};
};

export const actions: Actions = {
	/** Makes the event, as a draft or published, and goes to its own page: in edit mode for a draft. */
	save: async (event) => {
		const { viewer, party } = await partyFor(event);
		const form = await event.request.formData();

		return act(async () => {
			const input = await parseEventForm(event.locals.app, form);
			const uri = await event.locals.app.events.create(viewer, party.space, input);
			// create wrote it at this URI.
			const path = eventPagePath(party.skey, parseSpaceRecordUri(uri)!.rkey, input.name);

			redirect(303, input.publish ? path : `${path}?edit`);
		}, stringValues(form));
	}
};
