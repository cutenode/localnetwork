import { feeSummary, placeLabel } from '#lib/format.ts';
import type { MatchCriteria } from '#lib/matching.ts';
import type { ProjectService } from '#lib/server/services/projects.ts';
import type { ProjectView } from '#lib/types.ts';

/** How many of the newest listings to look through for one with enough filled in to show. */
const CANDIDATES = 12;

/** A real listing from the directory, as the landing pages show it, and the directory's totals. */
export async function directorySample(projects: ProjectService) {
	const [recent, stats] = await Promise.all([projects.recent(CANDIDATES), projects.stats()]);

	const isComplete = (project: ProjectView) =>
		!!project.profile.tagline && !!feeSummary(project.profile) && project.profile.genres.length > 0;

	return {
		example: recent.find(isComplete) ?? recent.at(0) ?? null,
		djs: stats.active,
		cities: stats.cityCount,
		unclaimed: stats.unclaimed
	};
}

/** The night a landing page asks the matcher about: the example listing's main sound, near its city. */
export function sampleNight(example: ProjectView) {
	const genre = example.profile.genres.at(0);

	if (!genre) return null;

	const { latE6, lngE6 } = example.profile.location;
	const hasLocation = latE6 !== undefined && lngE6 !== undefined;
	const location = hasLocation ? { lat: latE6 / 1e6, lng: lngE6 / 1e6 } : undefined;
	const place = placeLabel(example.profile);
	const criteria: MatchCriteria = { genres: [genre], location, radiusKm: 50 };

	// The same search on /match, so the full shortlist is a click away.
	const search = new URLSearchParams({ go: '1', genres: genre, radius: '50' });

	if (location) {
		search.set('place', place);
		search.set('lat', String(location.lat));
		search.set('lng', String(location.lng));
	}

	return { criteria, genre, place: location ? place : null, href: `/match?${search}` };
}
