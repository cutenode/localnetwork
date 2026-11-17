import { error, redirect } from '@sveltejs/kit';
import { claimTerms } from '#lib/requests.ts';
import { parseClaim } from '#lib/server/forms/claim.ts';
import { act, optionalText, requireFlag, requireViewer, stringValues } from '#lib/server/http.ts';
import type { Actions, PageServerLoad } from './$types';

/** The venue named by the address, for someone who runs it to ask staff to confirm them as its owner. */
async function claimed(locals: App.Locals, url: URL, key: string) {
	const viewer = requireViewer(locals, url);

	requireFlag(locals, 'peopleSpaces');
	const venue = await locals.app.venues.bySkey(key);

	if (!venue) error(404, 'Venue not found');

	return { viewer, venue };
}

export const load: PageServerLoad = async ({ params, locals, url }) => {
	const { viewer, venue } = await claimed(locals, url, params.key);

	return {
		venue: { space: venue.space, name: venue.name, locality: venue.address.locality },
		claimant: { did: viewer.did, handle: viewer.handle }
	};
};

export const actions: Actions = {
	default: async ({ request, params, locals, url }) => {
		const { viewer, venue } = await claimed(locals, url, params.key);
		const form = await request.formData();

		const result = await act(async () => {
			const submitted = await locals.app.requests.submit({
				actor: viewer,
				kind: 'claim',
				space: venue.space,
				claim: parseClaim(form, claimTerms({ space: venue.space })),
				message: optionalText(form, 'message', 20_000)
			});

			return { rkey: submitted.rkey };
		}, stringValues(form));

		if ('rkey' in result) redirect(303, `/dashboard/account/requests/${result.rkey}?submitted=1`);

		return result;
	}
};
