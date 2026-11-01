import { error, redirect } from '@sveltejs/kit';
import PerformerCard from '#lib/og/cards/PerformerCard.svelte';
import { performerCard, projectAvatar } from '#lib/server/og/cards.ts';
import { cardPaths } from '#lib/server/og/paths.ts';
import { renderCard } from '#lib/server/og/render.ts';
import { cards } from '#lib/server/og/serve.ts';
import type { RequestHandler } from './$types';

/** A performer's link-preview card. Only an active project has one, as only its page is public. */
export const GET: RequestHandler = async ({ params, locals }) => {
	const { app, log } = locals;
	const project = await app.projects.bySlug(params.slug);

	if (project?.status !== 'active') error(404, 'Not found');
	const path = cardPaths.project(project);

	if (params.cid !== project.cid) redirect(302, path);

	return cards.respond(log, path, async () =>
		renderCard(PerformerCard, performerCard(project, await projectAvatar(app.core, project, 360, log)))
	);
};
