import { unlessOffline } from '#lib/server/happyview/offline.ts';
import { directorySample, sampleNight } from '#lib/server/landing.ts';
import { pageMeta } from '#lib/server/meta.ts';
import type { PageServerLoad } from './$types';

/** How many matches the page shows from the matcher's answer, and how many it asks for to find them. */
const SHORTLIST = 3;

const CANDIDATES = 12;

export const load: PageServerLoad = ({ locals }) => {
	const { projects, profiles, geocoder, config } = locals.app;
	const viewer = locals.viewer;

	// Streamed, so the page paints before the directory answers: a real listing and the totals, then the matcher's real
	// shortlist for that listing's sound near its city. Null when the directory is offline.
	const directory = unlessOffline(locals.log, () => directorySample(projects));

	const shortlist = directory.then(async (found) => {
		if (!found) return null;

		const night = found.example && sampleNight(found.example);

		// A directory with nobody in it yet, or nobody with a sound to search on.
		if (!night) return { genre: null, place: null, href: '/match', results: [] };

		const ranked = await unlessOffline(locals.log, () => projects.match(night.criteria, CANDIDATES));

		// The matcher ranks every listed DJ; an example shows only the ones with a reason to book them.
		const results = ranked?.filter((result) => result.reasons.length > 0).slice(0, SHORTLIST);

		return results && { genre: night.genre, place: night.place, href: night.href, results };
	});

	// Whether a signed-in viewer already has the promoter section on, which changes the closing call.
	const promoting = viewer
		? unlessOffline(locals.log, async () => !!(await profiles.get(viewer.did))?.record.promoter)
		: Promise.resolve(false);

	return {
		geocoder: geocoder.enabled,
		directory,
		shortlist,
		promoting,
		meta: pageMeta(config.publicUrl, {
			title: 'For promoters',
			description:
				'Find DJs by sound, distance and budget, get a shortlist for your night, and book them in private threads.',
			path: '/for-promoters'
		})
	};
};
