import { isValidDid } from '@atproto/syntax';
import { error } from '@sveltejs/kit';
import { act, requireStaff } from '#lib/server/http.ts';
import type { Actions, PageServerLoad } from './$types';

/** Staff's view of one DJ in Find DJs: whether they're listed, their profile, and what they asked. */
export const load: PageServerLoad = async ({ params, locals, url }) => {
	requireStaff(locals, url);
	const { app } = locals;

	if (!isValidDid(params.did)) error(404, 'No account here');

	const [account, profile, listing, requests] = await Promise.all([
		app.accounts.view(params.did),
		app.profiles.get(params.did),
		app.djListings.get(params.did),
		app.requests.list({ kind: 'list', requester: params.did, limit: 20 })
	]);

	return { account, profile, listing, requests };
};

export const actions: Actions = {
	unlist: async ({ params, locals, url }) => {
		const viewer = requireStaff(locals, url);

		return act(async () => {
			await locals.app.djListings.unlist(viewer, params.did);

			return { success: 'Taken out of Find DJs' };
		});
	}
};
