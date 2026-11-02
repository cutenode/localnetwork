import { redirect } from '@sveltejs/kit';
import { parseAvatar, parseDetails, parseProfile, withTracklists } from '#lib/server/forms/project.ts';
import { formValues } from '#lib/project-form.ts';
import { act, requireAdmin, text } from '#lib/server/http.ts';
import type { Actions, PageServerLoad } from './$types';

export const load: PageServerLoad = ({ locals, url }) => {
	requireAdmin(locals, url);

	return { geocoder: locals.app.geocoder.enabled };
};

export const actions: Actions = {
	default: async ({ locals, url, request }) => {
		const viewer = requireAdmin(locals, url);
		const form = await request.formData();

		const result = await act(async () => {
			const managers = await Promise.all(
				text(form, 'managers', 2000)
					.split(/[\s,]+/)
					.filter(Boolean)
					.slice(0, 10)
					.map((value) => locals.app.identity.resolveIdentifier(value))
			);

			const { profile, tracklists } = await withTracklists(form, parseProfile(form));

			const project = await locals.app.projectAdmin.create({
				actor: viewer,
				profile,
				details: parseDetails(form),
				managers,
				avatar: await parseAvatar(form),
				tracklists
			});

			return { slug: project.slug };
		}, formValues(form));

		if ('slug' in result) redirect(303, `/admin/projects/${result.slug}?created=1`);

		return result;
	}
};
