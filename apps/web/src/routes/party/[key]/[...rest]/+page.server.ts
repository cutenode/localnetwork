import { error, redirect } from '@sveltejs/kit';
import { isListedRole } from '#lib/capabilities.ts';
import type { PageStatus } from '#lib/components/edit/types.ts';
import { openEditor, pageContext, savePage } from '#lib/server/edit-mode.ts';
import { parsePartyForm, partyValues } from '#lib/server/forms/party.ts';
import { cacheBriefly, requireFlag, requireViewer } from '#lib/server/http.ts';
import { pageMeta } from '#lib/server/meta.ts';
import { spacePagePath } from '#lib/space-paths.ts';
import type { Actions, PageServerLoad } from './$types';

/**
 * The party with this key, its people as the viewer may see them, and what the viewer can do on it. A draft is only
 * there for its people and staff; to anyone else it's not found, the same as a party that doesn't exist.
 */
async function findParty(locals: App.Locals, key: string) {
	requireFlag(locals, 'spacePages');
	const { app, viewer } = locals;
	const party = await app.parties.bySkey(key);

	if (!party) error(404, 'Not found');
	const people = await app.sharedSpaces.people(party.space, viewer);
	const person = people.find((someone) => someone.did === viewer?.did);
	const isHiddenDraft = !party.publishedAt && !person && !viewer?.isStaff;

	// A draft is only its people's and staff's to see.
	if (isHiddenDraft) error(404, 'Not found');

	return { party, people, can: person?.capabilities ?? [] };
}

/**
 * `/party/<key>/<name>`: the key finds the party and the name is only there to read (ADR 0016), so any other name, or
 * none, is sent on to the current one. Everything after the key is `rest`, so a later segment can name something
 * under the party.
 */
export const load: PageServerLoad = async (event) => {
	const { locals, params, url } = event;
	const { party, people, can } = await findParty(locals, params.key);
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
		const { locals, params, request, url } = event;
		const viewer = requireViewer(locals, url);
		const { party } = await findParty(locals, params.key);
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
	}
};
