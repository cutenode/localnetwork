import { error } from '@sveltejs/kit';
import { UserError } from '#lib/server/core.ts';
import { requireStaff } from '#lib/server/http.ts';
import type { PageServerLoad } from './$types';

/** A tracklist proposed in a moderation request, rendered for reviewers before they approve it. */
export const load: PageServerLoad = async ({ params, locals, url }) => {
	requireStaff(locals, url);
	const request = await locals.app.requests.byRkey(params.rkey);

	if (!request) error(404, 'Request not found');
	const n = Number(params.n);
	const mix = Number.isInteger(n) && n >= 1 ? request.record.profile?.mixes?.[n - 1] : undefined;

	if (!mix?.tracklist) error(404, 'This mix has no tracklist');

	try {
		const report = await locals.app.tracklists.proposed(request.repoDid, mix.tracklist);

		return { rkey: request.rkey, mix: { title: mix.title, url: mix.url }, report };
	} catch (cause) {
		if (cause instanceof UserError) error(cause.status, cause.message);
		throw cause;
	}
};
