import { error, redirect } from '@sveltejs/kit';
import { isListedRole } from '#lib/capabilities.ts';
import type { PageStatus } from '#lib/components/edit/types.ts';
import { openEditor, pageContext, savePage } from '#lib/server/edit-mode.ts';
import { answerEvent, changeEvent, eventPage, findEvent, LINK_TOKEN, saveEvent } from '#lib/server/event-page.ts';
import { parsePartyForm, partyValues } from '#lib/server/forms/party.ts';
import { cacheBriefly, requireViewer, text } from '#lib/server/http.ts';
import { pageMeta } from '#lib/server/meta.ts';
import { findParty, upcomingEvents } from '#lib/server/party-page.ts';
import { spacePagePath } from '#lib/space-paths.ts';
import type { Actions, PageServerLoad, RequestEvent } from './$types';

/** The party, and the event under it when what follows its key starts with one of its events' keys. */
async function find({ locals, params, url }: RequestEvent) {
	const found = await findParty(locals, params.key);

	return { found, shown: await findEvent(locals, found, params.rest, url.searchParams.get(LINK_TOKEN)) };
}

/**
 * `/party/<key>/<name>`: the key finds the party and the name is only there to read (ADR 0016), so any other name, or
 * none, is sent on to the current one. `/party/<key>/<event>/<name>` is one of its events (eventPage).
 */
export const load: PageServerLoad = async (event) => {
	const { locals, url } = event;
	const { found, shown } = await find(event);

	if (shown) return eventPage(event, found, shown);
	const { party, people, can, inside } = found;
	const { name, bio } = party.basics;
	const path = spacePagePath('party', party.skey, name);

	if (url.pathname !== path) redirect(308, `${path}${url.search}`);
	const space = await locals.app.sharedSpaces.get(party.space);
	const status: PageStatus = !party.publishedAt ? 'draft' : space?.listed ? 'listed' : 'unlisted';
	const context = pageContext(locals, 'party', party.space, can);

	cacheBriefly(event, 60, context.can ?? []);

	return {
		...context,
		editing: await openEditor(locals, url, context, name),
		version: party.cid,
		status,
		party: { basics: party.basics, avatarUrl: party.avatarUrl },
		events: await upcomingEvents(locals.app, party, inside),
		newEventHref: can.includes('events.manage') ? `/party/${party.skey}/new` : null,
		people: people
			.filter((person) => isListedRole('party', person.role) && !person.hidden)
			.map(({ did, handle, role }) => ({ did, handle, role })),
		geocoder: locals.app.geocoder.enabled,
		meta: pageMeta(locals.app.config.publicUrl, {
			title: name,
			description: bio ?? `${name}, a party on Local Network.`,
			path,
			image: party.avatarUrl ? { path: party.avatarUrl, alt: name } : undefined,
			noindex: !party.publishedAt
		})
	};
};

export const actions: Actions = {
	save: async (event) => {
		const { locals, request, url } = event;
		const { found, shown } = await find(event);

		if (shown) return saveEvent(event, shown);
		const viewer = requireViewer(locals, url);
		const { party } = found;
		const form = await request.formData();

		return savePage(
			event,
			form,
			async (version) =>
				locals.app.parties.updatePage(viewer, party.space, { ...(await parsePartyForm(form)), swap: version }),
			async () => {
				// SAFETY: a stale save read the party's record before refusing, and indexed every save before it.
				const latest = (await locals.app.parties.bySpace(party.space))!;

				return { version: latest.cid, values: partyValues(latest.basics) };
			}
		);
	},
	/** Cancels the event, over the version its page shows. */
	cancel: async (event) => {
		const { shown } = await find(event);

		if (!shown) error(404, 'Not found');

		return changeEvent(event, (viewer, form) =>
			event.locals.app.events.cancel(viewer, shown.uri, text(form, 'cid', 200))
		);
	},
	/** Going, Interested, or neither, for the signed-in viewer. */
	rsvp: async (event) => {
		const { shown } = await find(event);

		if (!shown) error(404, 'Not found');

		return answerEvent(event, shown);
	},
	/** Gives a private event a new link, so the old one stops opening it. */
	resetLink: async (event) => {
		const { shown } = await find(event);

		if (!shown) error(404, 'Not found');

		return changeEvent(event, (viewer) => event.locals.app.events.resetLink(viewer, shown.uri));
	}
};
