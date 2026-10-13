import { error, redirect } from '@sveltejs/kit';
import {
	formValues,
	parseAvatar,
	parseDetails,
	parseProfile,
	profileToValues,
	withTracklists
} from '#lib/server/forms/project.ts';
import { act, optionalText, requireViewer } from '#lib/server/http.ts';
import type { Actions, PageServerLoad } from './$types';

export const load: PageServerLoad = async ({ params, locals, url }) => {
	const viewer = requireViewer(locals, url);
	const project = await locals.app.projects.bySlug(params.slug);

	if (!project) error(404, 'Project not found');

	if (!project.managers.includes(viewer.did)) error(403, 'Only managers of this project can propose changes');
	// Keep editing the request that's already open instead of starting over from the published profile.
	const open = await locals.app.requests.openUpdate(project.uri, viewer.did);

	if (open) redirect(303, `/me/requests/${open.rkey}`);
	const details = await locals.app.projects.details(project.uri);

	return {
		project,
		values: profileToValues(project.profile, details),
		geocoder: locals.app.geocoder.enabled,
		// Streamed, so the form shows while HappyView reads the managers' sets.
		sets: locals.app.sets.published(project.managers)
	};
};

export const actions: Actions = {
	default: async ({ request, params, locals, url }) => {
		const viewer = requireViewer(locals, url);
		const project = await locals.app.projects.bySlug(params.slug);

		if (!project) error(404, 'Project not found');
		const form = await request.formData();

		const result = await act(async () => {
			let profile = parseProfile(form);

			if (form.get('removeAvatar') !== 'on' && project.profile.avatar)
				profile = { ...profile, avatar: project.profile.avatar };
			const withFiles = await withTracklists(form, profile, [project.profile]);

			const submitted = await locals.app.requests.submit({
				actor: viewer,
				kind: 'update',
				projectUri: project.uri,
				profile: withFiles.profile,
				details: parseDetails(form),
				avatar: await parseAvatar(form),
				tracklists: withFiles.tracklists,
				message: optionalText(form, 'message', 20_000)
			});

			return { rkey: submitted.rkey };
		}, formValues(form));

		if ('rkey' in result) redirect(303, `/me/requests/${result.rkey}?submitted=1`);

		return result;
	}
};
