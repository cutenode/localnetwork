import { error, redirect } from '@sveltejs/kit';
import { diff } from '#lib/server/services/diff.ts';
import { parseAvatar, parseDetails, parseProfile, profileToValues, withTracklists } from '#lib/server/forms/project.ts';
import { formValues } from '#lib/project-form.ts';
import { parseClaim } from '#lib/server/forms/claim.ts';
import { act, optionalText, requireViewer, stringValues } from '#lib/server/http.ts';
import type { Actions, PageServerLoad } from './$types';

async function loadOwn(locals: App.Locals, url: URL, rkey: string) {
	const viewer = requireViewer(locals, url);
	const request = await locals.app.requests.byRkey(rkey);

	if (!request || (request.requester.did !== viewer.did && !viewer.isStaff)) error(404, 'Request not found');

	return { viewer, request };
}

export const load: PageServerLoad = async ({ params, locals, url }) => {
	const { request } = await loadOwn(locals, url, params.rkey);
	const project = request.projectUri ? await locals.app.projects.byUri(request.projectUri) : null;

	const changes =
		request.kind === 'update' && project
			? diff({ profile: project.profile }, { profile: request.record.profile ?? project.profile })
			: [];

	const avatar = request.record.profile?.avatar;

	return {
		request,
		project,
		changes,
		// A photo kept from the published profile is root's; a new one is still on the requester's PDS.
		avatarUrl: !avatar
			? null
			: avatar.ref.$link === project?.profile.avatar?.ref.$link
				? project.avatarUrl
				: `/dashboard/requests/${request.rkey}/avatar`,
		values: profileToValues(request.record.profile, request.record.details),
		submitted: url.searchParams.has('submitted'),
		geocoder: locals.app.geocoder.enabled,
		// Only managers can link sets, so a new project's form has no picker. Streamed, so the form shows meanwhile.
		sets: request.kind === 'update' && project ? locals.app.sets.published(project.managers) : undefined
	};
};

export const actions: Actions = {
	withdraw: async ({ params, locals, url }) => {
		const { viewer, request } = await loadOwn(locals, url, params.rkey);

		return act(async () => {
			await locals.app.requests.withdraw(viewer, request.uri);

			return { success: 'Request withdrawn' };
		});
	},
	revise: async ({ params, locals, url, request: httpRequest }) => {
		const { viewer, request } = await loadOwn(locals, url, params.rkey);
		const form = await httpRequest.formData();

		const result = await act(
			async () => {
				if (request.kind === 'claim') {
					await locals.app.requests.revise(viewer, request.uri, {
						claim: parseClaim(form),
						message: optionalText(form, 'message', 20_000) ?? ''
					});
				} else {
					let profile = parseProfile(form);

					if (form.get('removeAvatar') !== 'on' && request.record.profile?.avatar)
						profile = { ...profile, avatar: request.record.profile.avatar };
					const withFiles = await withTracklists(form, profile, [request.record.profile]);
					await locals.app.requests.revise(viewer, request.uri, {
						profile: withFiles.profile,
						details: parseDetails(form),
						avatar: await parseAvatar(form),
						tracklists: withFiles.tracklists,
						message: optionalText(form, 'message', 20_000) ?? ''
					});
				}

				return { revised: true };
			},
			request.kind === 'claim' ? stringValues(form) : formValues(form)
		);

		if ('revised' in result) redirect(303, `/dashboard/requests/${params.rkey}`);

		return result;
	}
};
