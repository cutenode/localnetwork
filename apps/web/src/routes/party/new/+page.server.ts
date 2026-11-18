import { redirect } from '@sveltejs/kit';
import { capabilitiesFor } from '#lib/capabilities.ts';
import type { PageContext } from '#lib/components/edit/types.ts';
import { parseSpaceUri } from '#lib/server/atproto/spaces.ts';
import { parsePartyForm } from '#lib/server/forms/party.ts';
import { act, requireFlag, requireViewer, stringValues } from '#lib/server/http.ts';
import { pageMeta } from '#lib/server/meta.ts';
import { spacePagePath } from '#lib/space-paths.ts';
import type { Actions, PageServerLoad } from './$types';

/**
 * A party being started: its page, empty and in edit mode, as a draft. Whoever starts it becomes its host, so the page
 * offers what a host can do.
 */
export const load: PageServerLoad = async ({ locals, url }) => {
	requireFlag(locals, 'spacePages');
	requireViewer(locals, url);

	const context: PageContext = {
		subject: { kind: 'party', uri: 'party:new' },
		can: capabilitiesFor('party', 'host'),
		role: 'owner'
	};

	return {
		...context,
		geocoder: locals.app.geocoder.enabled,
		meta: pageMeta(locals.app.config.publicUrl, {
			title: 'New party',
			description: 'Start a party on Local Network.',
			path: '/party/new',
			noindex: true
		})
	};
};

export const actions: Actions = {
	/** Starts the party, as a draft or published, and goes to its own page: in edit mode for a draft. */
	save: async ({ locals, request, url }) => {
		requireFlag(locals, 'spacePages');
		const viewer = requireViewer(locals, url);
		const form = await request.formData();

		return act(async () => {
			const input = await parsePartyForm(form);
			const space = await locals.app.parties.start(viewer, input);
			// start made the space, so its URI parses.
			const path = spacePagePath('party', parseSpaceUri(space)!.skey, input.basics.name);

			redirect(303, input.publish ? path : `${path}?edit`);
		}, stringValues(form));
	}
};
