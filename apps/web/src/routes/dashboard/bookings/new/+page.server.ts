import { error, redirect } from '@sveltejs/kit';
import { parseShow } from '#lib/server/forms/show.ts';
import { act, requireViewer, stringValues, text } from '#lib/server/http.ts';
import { inquiryDefaults, inquiryValues } from '#lib/shows.ts';
import type { Viewer } from '#lib/types.ts';
import type { Actions, PageServerLoad } from './$types';

/** The viewer's planner show `rkey`, to start the inquiry from, or null when it can't be read. */
async function plannerShow(locals: App.Locals, viewer: Viewer, rkey: string) {
	if (!rkey || !viewer.spacesCapable) return null;

	try {
		return await locals.app.planner.show(viewer, rkey);
	} catch (cause) {
		// The form still opens, just without the show's details.
		locals.log.warn({ err: cause, rkey }, 'planner show unavailable for inquiry');

		return null;
	}
}

export const load: PageServerLoad = async ({ locals, url }) => {
	const viewer = requireViewer(locals, url);
	const project = await locals.app.projects.bySlug(url.searchParams.get('project') ?? '');

	if (!project || project.status !== 'active') error(404, 'Pick a project to contact first');

	if (project.managers.includes(viewer.did)) redirect(303, `/projects/${project.slug}`);

	// From the planner, the show's details go in; from the matcher, only its date.
	const show = await plannerShow(locals, viewer, url.searchParams.get('show') ?? '');

	const values = show
		? inquiryValues(show.details)
		: { ...inquiryDefaults, date: url.searchParams.get('date')?.slice(0, 10) ?? '' };

	return { project, values, show: show && { rkey: show.rkey, title: show.details.title } };
};

export const actions: Actions = {
	default: async ({ request, locals, url }) => {
		const viewer = requireViewer(locals, url);
		const project = await locals.app.projects.bySlug(url.searchParams.get('project') ?? '');

		if (!project) error(404, 'Project not found');
		const form = await request.formData();

		const result = await act(async () => {
			const space = await locals.app.bookings.start({
				actor: viewer,
				projectUri: project.uri,
				show: parseShow(form),
				message: text(form, 'message', 20_000)
			});

			return { skey: space.split('/').at(-1)! };
		}, stringValues(form));

		if ('skey' in result) redirect(303, `/dashboard/bookings/${result.skey}`);

		return result;
	}
};
