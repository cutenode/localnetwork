import type { ShowsPlace } from '#lib/server/shows-place.ts';
import type { FollowState, ProjectView, ShowView } from '#lib/types.ts';

/** An event as the home page lists it: the shows on its calendar event, which are its lineup, in the order listed. */
export type HomeEvent = { uri: string; shows: [ShowView, ...ShowView[]] };

/**
 * Groups shows into events by the calendar event each names, keeping the order of each event's first show, so a
 * soonest-first list stays soonest first.
 */
export function eventsOf(shows: ShowView[]): HomeEvent[] {
	const events = new Map<string, HomeEvent>();

	for (const show of shows) {
		const uri = show.record.event.uri;
		const event = events.get(uri);

		if (event) event.shows.push(show);
		else events.set(uri, { uri, shows: [show] });
	}

	return [...events.values()];
}

/**
 * Where the home page lists shows and DJs near, and what's there: each list null when the directory couldn't answer,
 * and each DJ's followers null when those couldn't be counted. Neither has what the viewer's network already shows.
 */
export type HomeArea = {
	place: ShowsPlace;
	shows: ShowView[] | null;
	djs: { project: ProjectView; follow: FollowState | null }[] | null;
};
