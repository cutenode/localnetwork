import { error } from '@sveltejs/kit';

/**
 * What a project's public pages (its profile and its press kit) both load: the project, 404 when
 * it's hidden from this viewer, the booking details for signed-in members, its managers, and the
 * managers' own DJ records, streamed so a slow PDS never holds the page up. Records already at hand
 * (or no managers to ask) render with the page, so no placeholder comes and goes and moves what's
 * below it.
 */
export async function loadProjectPage(locals: App.Locals, slug: string) {
	const { app, viewer } = locals;
	const project = await app.projects.bySlug(slug);

	if (!project) error(404, 'No project with that name');
	const isManager = !!viewer && project.managers.includes(viewer.did);

	if (project.status !== 'active' && !isManager && !viewer?.isStaff) error(404, 'No project with that name');

	const [details, accounts] = await Promise.all([
		viewer ? app.projects.details(project.uri) : Promise.resolve(null),
		app.accounts.views(project.managers)
	]);

	const managers = project.managers.map((did) => accounts.get(did) ?? { did, handle: null });

	return { project, details, managers, isManager, dj: app.dj.fresh(managers) ?? app.dj.activity(managers) };
}
