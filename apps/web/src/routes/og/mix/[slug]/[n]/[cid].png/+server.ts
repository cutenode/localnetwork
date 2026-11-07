import { error, redirect } from '@sveltejs/kit';
import MixCard from '#lib/og/cards/MixCard.svelte';
import { UserError } from '#lib/server/core.ts';
import { PHOTO_TYPES } from '#lib/server/images.ts';
import { cardImage, dataUrlBytes, projectAvatar } from '#lib/server/og/cards.ts';
import { cardPaths } from '#lib/server/og/paths.ts';
import { renderCard } from '#lib/server/og/render.ts';
import { cards } from '#lib/server/og/serve.ts';
import type { RequestHandler } from './$types';

const COVER = 300;

/** A mix's link-preview card, for the nth mix with a tracklist of an active project. */
export const GET: RequestHandler = async ({ params, locals }) => {
	const { app, log } = locals;
	const project = await app.projects.bySlug(params.slug);

	if (project?.status !== 'active') error(404, 'Not found');
	const n = Number(params.n);
	const mix = Number.isInteger(n) && n >= 1 ? project.profile.mixes?.[n - 1] : undefined;
	const tracklist = mix?.tracklist;

	if (!mix || !tracklist) error(404, 'Not found');
	const path = cardPaths.mix(project, n);

	if (params.cid !== project.cid) redirect(302, path);

	try {
		return await cards.respond(log, path, async () => {
			const report = await app.tracklists.published(tracklist);
			// A report's cover takes the same types as a press photo (mix/schema.ts).
			const cover = report.mix.cover && dataUrlBytes(report.mix.cover);
			const embedded = cover && (await cardImage(cover, { width: COVER, height: COVER, accept: PHOTO_TYPES }, log));

			return renderCard(MixCard, {
				title: mix.title,
				performer: project.profile.name,
				durationSecs: report.mix.durationSecs,
				tracks: report.plays.length,
				// Without a cover of its own, the performer's picture.
				cover: embedded || (await projectAvatar(app.core, project, COVER, log)),
				waveform: report.waveform
			});
		});
	} catch (cause) {
		if (cause instanceof UserError) error(cause.status, cause.message);
		throw cause;
	}
};
