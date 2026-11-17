import { slimCapture } from '#lib/components/tools/sources.ts';
import { unlessOffline } from '#lib/server/happyview/offline.ts';
import { directorySample } from '#lib/server/landing.ts';
import { pageMeta } from '#lib/server/meta.ts';
import { chartFilters } from '#lib/supports.ts';
import { findTool } from '#lib/tools/index.ts';
import type { PageServerLoad } from './$types';

/** How many upcoming shows and most played tracks the page shows. */
const SHOWS = 3;

const TRACKS = 5;

/** The locallibrary captures the page shows, by the name of their hot cue on its tool page. */
const CUES = ['Sync', 'Tracklist'];

export const load: PageServerLoad = ({ locals }) => {
	const { projects, shows, supports, config } = locals.app;

	return {
		// Streamed, so the page paints before the directory answers: a real listing as promoters see it with the
		// directory's totals, the next shows and this month's most played tracks. Each is null when it's offline.
		directory: unlessOffline(locals.log, () => directorySample(projects)),
		shows: unlessOffline(locals.log, async () => (await shows.upcoming({ limit: SHOWS })).items),
		tracks: unlessOffline(locals.log, async () =>
			(await supports.charts(chartFilters(new URLSearchParams(), new Date()))).tracks.slice(0, TRACKS)
		),
		captures: (findTool('locallibrary')?.cues ?? [])
			.filter((cue) => CUES.includes(cue.name))
			.map((cue) => ({ line: cue.line, capture: slimCapture(cue.capture) })),
		meta: pageMeta(config.publicUrl, {
			title: 'For DJs',
			description: 'Get listed, or claim the listing an admin made, so promoters near you can find and book you.',
			path: '/for-djs'
		})
	};
};
