import { redirect } from '@sveltejs/kit';
import { formValues, parseAvatar, parseDetails, parseProfile, withTracklists } from '#lib/server/forms/project.ts';
import { act, optionalText, requireViewer } from '#lib/server/http.ts';
import type { Actions, PageServerLoad } from './$types';

export const load: PageServerLoad = ({ locals, url }) => {
	requireViewer(locals, url);

	return { geocoder: locals.app.geocoder.enabled };
};

export const actions: Actions = {
	default: async ({ request, locals, url }) => {
		const viewer = requireViewer(locals, url);
		const form = await request.formData();

		const result = await act(async () => {
			const { profile, tracklists } = await withTracklists(form, parseProfile(form));

			const submitted = await locals.app.requests.submit({
				actor: viewer,
				kind: 'create',
				profile,
				details: parseDetails(form),
				avatar: await parseAvatar(form),
				tracklists,
				message: optionalText(form, 'message', 20_000)
			});

			return { rkey: submitted.rkey };
		}, formValues(form));

		if ('rkey' in result) redirect(303, `/dashboard/requests/${result.rkey}?submitted=1`);

		return result;
	}
};
