import type { Cookies } from '@sveltejs/kit';
import { chartFilters } from '#lib/supports.ts';
import type { HomeArea } from '#lib/home-events.ts';
import type { ShowView, SupportedTrack, Viewer } from '#lib/types.ts';
import type { App } from './app';
import { unlessOffline } from './happyview/offline';
import type { Logger } from './logger';
import { type ShowsPlace, showsPlace } from './shows-place';

/** How many upcoming shows each list asks for, DJs from the area it shows, and most played tracks. */
const SHOWS = 12;

const DJS = 6;

const TRACKS = 5;

/** How far around a home city counts as near it, as /shows's default radius. */
const HOME_RADIUS_KM = 50;

type Locals = { app: App; viewer: Viewer | null; log: Pick<Logger, 'warn'> };

/**
 * Upcoming shows from `did`'s network, soonest first. Today a network is the acts someone follows, which listShows
 * joins on. Following people (LOC-391) adds their shows here: the acts a followed person plays in or manages.
 */
export const networkShows = ({ app, log }: Locals, did: string) =>
	unlessOffline(log, async () => (await app.shows.upcoming({ follower: did, limit: SHOWS })).items);

/** The viewer's home city, from their profile, as a place shows can be near. Null without one or its coordinates. */
async function homeCity({ app, log }: Locals, did: string): Promise<ShowsPlace | null> {
	try {
		const location = (await app.profiles.get(did))?.record.basics.location;

		if (location?.latE6 === undefined || location.lngE6 === undefined) return null;

		return { place: location.city, lat: location.latE6 / 1e6, lng: location.lngE6 / 1e6, radiusKm: HOME_RADIUS_KM };
	} catch (error) {
		log.warn("couldn't read the viewer's home city", { err: error });

		return null;
	}
}

/**
 * Shows and DJs near `place`, with each DJ's followers. It leaves out the shows already listed in `listed`, and the DJs
 * the viewer follows, so what's near them is something new.
 */
async function area(
	{ app, viewer, log }: Locals,
	place: ShowsPlace,
	listed: Promise<ShowView[] | null>
): Promise<HomeArea> {
	const near = { lat: place.lat, lng: place.lng };

	const [shows, djs, already] = await Promise.all([
		unlessOffline(log, async () => (await app.shows.upcoming({ near, radiusKm: place.radiusKm, limit: SHOWS })).items),
		unlessOffline(
			log,
			async () => (await app.projects.search({ near, radiusKm: place.radiusKm, sort: 'distance', pageSize: DJS })).items
		),
		listed
	]);

	const seen = new Set(already?.map((show) => show.uri));

	const followed = djs
		? await Promise.all(
				djs.map(async (project) => ({
					project,
					follow: await unlessOffline(log, () => app.follows.state(project, viewer?.did))
				}))
			)
		: null;

	return {
		place,
		shows: shows?.filter((show) => !seen.has(show.uri)) ?? null,
		djs: followed?.filter(({ follow }) => !follow?.following) ?? null
	};
}

/**
 * The social home page's data. Signed out, it lists what's near the place picked last (here or on /shows). Signed in,
 * it leads with the viewer's network, then what's near their home city, or the place picked when they have none.
 * Everything from the directory streams, so the page paints first.
 */
export function socialHome(locals: Locals, url: URL, cookies: Pick<Cookies, 'get' | 'set' | 'delete'>) {
	const { app, viewer, log } = locals;
	const picked = showsPlace(url, cookies, app.config.publicUrl.startsWith('https:'));
	const network = viewer ? networkShows(locals, viewer.did) : null;
	const place = viewer ? homeCity(locals, viewer.did).then((home) => home ?? picked) : Promise.resolve(picked);

	return {
		picked,
		network,
		area: place.then((found) => (found ? area(locals, found, network ?? Promise.resolve(null)) : null)),
		tracks: unlessOffline(log, async (): Promise<SupportedTrack[]> =>
			(await app.supports.charts(chartFilters(new URLSearchParams(), new Date()))).tracks.slice(0, TRACKS)
		)
	};
}
