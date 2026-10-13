import { error } from '@sveltejs/kit';
import {
	formValues,
	parseAvatar,
	parseDetails,
	parseProfile,
	profileToValues,
	withTracklists
} from '#lib/server/forms/project.ts';
import { act, requireAdmin, requireStaff, text } from '#lib/server/http.ts';
import { isProjectStatus } from '#lib/server/services/projects.ts';
import type { Actions, PageServerLoad } from './$types';

export const load: PageServerLoad = async ({ params, locals, url }) => {
	const project = await locals.app.projects.bySlug(params.slug);

	if (!project) error(404, 'Project not found');

	const [details, managers, trail, requests] = await Promise.all([
		locals.app.projects.details(project.uri),
		locals.app.accounts.views(project.managers),
		locals.app.audit.list({ subject: project.uri, limit: 30 }),
		locals.app.requests.list({ projectUri: project.uri, limit: 30 })
	]);

	return {
		project,
		values: profileToValues(project.profile, details),
		managers: project.managers.map((did) => managers.get(did) ?? { did, handle: null }),
		trail,
		requests,
		created: url.searchParams.has('created'),
		geocoder: locals.app.geocoder.enabled,
		// Streamed, so the form shows while HappyView reads the managers' sets.
		sets: locals.app.sets.published(project.managers)
	};
};

export const actions: Actions = {
	update: async ({ params, locals, url, request }) => {
		const viewer = requireAdmin(locals, url);
		const project = await locals.app.projects.bySlug(params.slug);

		if (!project) error(404, 'Project not found');
		const form = await request.formData();

		return act(async () => {
			const { profile, tracklists } = await withTracklists(form, parseProfile(form), [project.profile]);
			await locals.app.projectAdmin.update({
				actor: viewer,
				project,
				profile,
				details: parseDetails(form),
				avatar: await parseAvatar(form),
				removeAvatar: form.get('removeAvatar') === 'on',
				tracklists
			});

			return { success: 'Saved and published' };
		}, formValues(form));
	},
	status: async ({ params, locals, url, request }) => {
		const viewer = requireStaff(locals, url);
		const project = await locals.app.projects.bySlug(params.slug);

		if (!project) error(404, 'Project not found');
		const form = await request.formData();

		return act(async () => {
			const status = text(form, 'status', 16);

			if (!isProjectStatus(status)) return { error: 'Unknown status' };
			await locals.app.projectAdmin.setStatus({
				actor: viewer,
				project,
				status,
				reason: text(form, 'reason', 1000) || undefined
			});

			return { success: `Status set to ${status}` };
		});
	},
	managers: async ({ params, locals, url, request }) => {
		const viewer = requireAdmin(locals, url);
		const project = await locals.app.projects.bySlug(params.slug);

		if (!project) error(404, 'Project not found');
		const form = await request.formData();

		return act(async () => {
			const keep = form
				.getAll('keep')
				.map(String)
				.filter((did) => project.managers.includes(did));

			const added = text(form, 'add', 300);
			const managers = added ? [...keep, await locals.app.identity.resolveIdentifier(added)] : keep;
			await locals.app.projectAdmin.setManagers({ actor: viewer, project, managers });

			return { success: 'Managers updated' };
		});
	}
};
