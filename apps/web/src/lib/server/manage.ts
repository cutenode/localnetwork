import { error } from '@sveltejs/kit';
import type { Viewer } from '#lib/types.ts';
import { requireViewer } from './http';
import type { ProjectService } from './services/projects';

/**
 * The project `slug` and the signed-in viewer, when the viewer manages it: a 404 when there's no
 * such project, a 403 when they don't manage it. The dashboard's project layout checks this for
 * its pages, and each form action checks it again, since actions run without the layout.
 */
export async function requireManager(
	locals: { viewer: Viewer | null; app: { projects: Pick<ProjectService, 'bySlug'> } },
	url: URL,
	slug: string
) {
	const viewer = requireViewer(locals, url);
	const project = await locals.app.projects.bySlug(slug);

	if (!project) error(404, 'No project with that name');

	if (!project.managers.includes(viewer.did)) error(403, "Only this project's managers can manage it");

	return { viewer, project };
}
