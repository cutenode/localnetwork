import { feeSummary, kindLabel, profileHref } from './format';
import type { ListedDj, ProjectView } from './types';

/** What a performer's card shows, whether a project or a listed DJ. */
export type PerformerSummary = {
	href: string;
	name: string;
	avatarUrl: string | null;
	/** What they are, like Solo DJ or Duo. */
	kind: string;
	place: string | null;
	distanceKm: number | null;
	/** A project nobody has claimed yet. */
	unclaimed: boolean;
	tagline?: string;
	genres: string[];
	fee: string;
};

export function projectSummary(project: ProjectView): PerformerSummary {
	const { profile } = project;

	return {
		href: `/projects/${project.slug}`,
		name: profile.name,
		avatarUrl: project.avatarUrl,
		kind: kindLabel(profile.kind),
		place: `${profile.location.city}, ${profile.location.country}`,
		distanceKm: project.distanceKm ?? null,
		unclaimed: project.managers.length === 0,
		tagline: profile.tagline,
		genres: profile.genres,
		fee: feeSummary(profile)
	};
}

/** A listed DJ, a solo act by their own name, with the genres the directory found them by. */
export function djSummary(dj: ListedDj): PerformerSummary {
	const { basics, performer = {} } = dj.profile.record;

	return {
		href: profileHref({ did: dj.profile.did, handle: dj.handle }),
		name: basics.name,
		avatarUrl: dj.profile.avatarUrl,
		kind: kindLabel('solo'),
		place: basics.location ? `${basics.location.city}, ${basics.location.country}` : null,
		distanceKm: dj.distanceKm ?? null,
		unclaimed: false,
		tagline: performer.tagline,
		genres: dj.genres,
		fee: feeSummary(performer)
	};
}
