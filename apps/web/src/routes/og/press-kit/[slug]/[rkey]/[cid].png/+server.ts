import { error, redirect } from '@sveltejs/kit';
import PressKitCard from '#lib/og/cards/PressKitCard.svelte';
import { CARD } from '#lib/og/style.ts';
import { PHOTO_TYPES } from '#lib/server/images.ts';
import { cardImage, performerCard, projectAvatar } from '#lib/server/og/cards.ts';
import { cardPaths } from '#lib/server/og/paths.ts';
import { renderCard } from '#lib/server/og/render.ts';
import { cards } from '#lib/server/og/serve.ts';
import type { RequestHandler } from './$types';

/**
 * A press kit's link-preview card: its first public photo (never one from the private part), or the performer's card
 * without one. Its CID is the press kit's, which changes on every publish.
 */
export const GET: RequestHandler = async ({ params, locals }) => {
	const { app, log } = locals;
	const project = await app.projects.bySlug(params.slug);
	const kit = project?.status === 'active' ? await app.epks.get(project, params.rkey) : null;

	if (!project || !kit) error(404, 'Not found');
	const path = cardPaths.pressKit(project.slug, kit.epk);

	if (params.cid !== kit.epk.cid) redirect(302, path);

	return cards.respond(log, path, async () => {
		const first = kit.epk.record.photos?.[0];
		const photo = first && (await app.epks.photo(kit.epk, first.image.ref.$link, false));
		const drawn = photo && (await cardImage(photo.bytes, { ...CARD, accept: PHOTO_TYPES }, log));

		return renderCard(PressKitCard, {
			performer: performerCard(project, drawn ? null : await projectAvatar(app.core, project, 360, log)),
			headline: kit.epk.record.headline ?? null,
			photo: drawn || null
		});
	});
};
