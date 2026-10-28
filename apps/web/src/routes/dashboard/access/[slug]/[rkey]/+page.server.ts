import { error, redirect } from '@sveltejs/kit';
import { pressKitPath } from '#lib/epk.ts';
import { act, requireViewer, text } from '#lib/server/http.ts';
import type { Actions, PageServerLoad, RequestEvent } from './$types';

/** The press kit someone asks to see, by its project and key; a 404 for one of a project they can't see. */
async function askedKit({ locals, params, url }: Pick<RequestEvent, 'locals' | 'params' | 'url'>) {
	const viewer = requireViewer(locals, url);
	const project = await locals.app.projects.bySlug(params.slug);
	const visible = project && (project.status === 'active' || project.managers.includes(viewer.did) || viewer.isStaff);
	const kit = visible ? await locals.app.epks.get(project, params.rkey) : null;

	if (!kit) error(404, 'Press kit not found');

	return { viewer, kit };
}

export const load: PageServerLoad = async (event) => {
	const { viewer, kit } = await askedKit(event);
	const access = await event.locals.app.epks.accessState(kit, viewer);

	// The project's managers decide who gets in, which happens on its own dashboard page.
	if (access.kind === 'manager')
		redirect(303, `/dashboard/projects/${kit.project.slug}/press-kits/${kit.epk.rkey}/access`);

	return { epk: kit.epk, project: kit.project, access };
};

export const actions: Actions = {
	request: async (event) => {
		const { viewer, kit } = await askedKit(event);
		const form = await event.request.formData();

		const result = await act(async () => {
			const state = await event.locals.app.epks.request(viewer, kit, {
				role: text(form, 'role', 32),
				message: text(form, 'message', 1000)
			});

			return { requested: state.kind };
		});

		// Let in at once by the press kit's rules, so straight back to all of it.
		if ('requested' in result && result.requested === 'granted')
			redirect(303, pressKitPath(kit.project.slug, kit.epk.rkey));

		return result;
	}
};
