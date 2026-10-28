import { redirect } from '@sveltejs/kit';
import { saveDraftForm } from '#lib/server/epk-page.ts';
import { epkFormValues, submittedValues } from '#lib/server/forms/epk.ts';
import { act } from '#lib/server/http.ts';
import { requireManager } from '#lib/server/manage.ts';
import type { Actions, PageServerLoad, RequestEvent } from './$types';

export const load: PageServerLoad = async ({ parent }) => {
	const { project } = await parent();

	return { values: epkFormValues(project.profile.mixes ?? []) };
};

/** Saves the form as the viewer's draft of a new press kit, and publishes it with `publish`; the form comes back on an error. */
async function fromForm({ request, locals, url, params }: RequestEvent, publish: boolean) {
	const { viewer, project } = await requireManager(locals, url, params.slug);
	const form = await request.formData();
	const { epks } = locals.app;

	const result = await act(
		async () => {
			const draft = await saveDraftForm(epks, viewer, project, form, []);

			if (!draft) throw new Error('a new press kit always differs from nothing');

			if (!publish) return { rkey: draft.rkey };
			const published = await epks.drafts.publish(viewer, project, draft.rkey);

			// A new press kit gets a key of the root's choosing, not its draft's.
			return { rkey: published.kind === 'published' ? published.epk.rkey : draft.rkey };
		},
		submittedValues(form, project.profile.mixes ?? [])
	);

	if ('rkey' in result)
		redirect(
			303,
			`/dashboard/projects/${project.slug}/press-kits/${result.rkey}?notice=${publish ? 'published' : 'saved'}`
		);

	return result;
}

export const actions: Actions = {
	save: (event) => fromForm(event, false),
	publish: (event) => fromForm(event, true)
};
