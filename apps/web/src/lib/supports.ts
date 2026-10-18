import type { ArtistCredit, DirectoryPlace } from './types.ts';

/**
 * Who played whose tracks: the artist pages (`/artists/<id>`) and the charts (`/charts`), counted by the directory's
 * getArtist and getCharts. Artists and tracks are known by their MusicBrainz IDs when sets name them.
 */

const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/;

/**
 * A MusicBrainz ID as the bare lowercase UUID the directory files it under. Sets write `mbid:<uuid>` or the bare UUID
 * (teal.fm's form), in either case. Undefined when it's neither.
 */
export function bareMbid(value: string): string | undefined {
	const id = value.toLowerCase().replace(/^mbid:/, '');

	return UUID.test(id) ? id : undefined;
}

/** The artists a set's track credits: one by one when the set tells them apart, else its whole credit as one. */
export function trackArtists(track: {
	artist?: string;
	artists?: { artistName: string; artistMbId?: string }[];
}): ArtistCredit[] {
	if (track.artists?.length)
		return track.artists.map((artist) => ({ name: artist.artistName, mbid: artist.artistMbId }));

	return track.artist ? [{ name: track.artist }] : [];
}

/**
 * An artist's page: `/artists/<mbid>` when they have one (its canonical address), else `/artists/name/<name>`. A
 * path can't carry a name that's only dots (URLs resolve `.` and `..`, however they're encoded), so those go in the
 * query.
 */
export function artistHref(artist: ArtistCredit): string {
	if (artist.mbid) return `/artists/${artist.mbid}`;

	const name = encodeURIComponent(artist.name);

	return /^\.{1,2}$/.test(artist.name) ? `/artists/name?name=${name}` : `/artists/name/${name}`;
}

export const musicBrainzUrl = (kind: 'artist' | 'recording', mbid: string) => `https://musicbrainz.org/${kind}/${mbid}`;

/** How far back the charts look unless a month is picked. */
export const CHART_DAYS = 30;

const day = (date: Date) => date.toISOString().slice(0, 10);

const MONTH = /^(\d{4})-(0[1-9]|1[0-2])$/;

/** "October 2026" for 2026-10. */
export const monthLabel = (month: string) =>
	new Intl.DateTimeFormat('en', { month: 'long', year: 'numeric', timeZone: 'UTC' }).format(
		new Date(`${month}-01T00:00:00Z`)
	);

/** The months the charts offer, this one first, as YYYY-MM. */
export function recentMonths(today: Date, count = 12): string[] {
	return Array.from({ length: count }, (_, i) =>
		day(new Date(Date.UTC(today.getUTCFullYear(), today.getUTCMonth() - i, 1))).slice(0, 7)
	);
}

/** "Berlin, DE", the form a place takes in the charts' `place` param. */
export const placeValue = (place: Pick<DirectoryPlace, 'city' | 'country'>) => `${place.city}, ${place.country}`;

export type ChartFilters = {
	/** YYYY-MM, or null for the last CHART_DAYS days. */
	month: string | null;
	/** "Berlin, DE", or null for everywhere. */
	place: string | null;
	/** What getCharts gets. */
	query: { since: string; before?: string; city?: string; country?: string };
};

/** The charts' filters from the page's `month` and `place` params, falling back to the last 30 days everywhere. */
export function chartFilters(params: URLSearchParams, today: Date): ChartFilters {
	const monthParam = MONTH.exec(params.get('month') ?? '');
	const placeParam = /^(.{1,200}), ([A-Za-z]{2})$/.exec(params.get('place')?.trim() ?? '');
	const place = placeParam && { city: placeParam[1], country: placeParam[2].toUpperCase() };

	const period = monthParam
		? {
				since: `${monthParam[0]}-01`,
				before: day(new Date(Date.UTC(Number(monthParam[1]), Number(monthParam[2]), 1)))
			}
		: {
				since: day(new Date(today.getTime() - (CHART_DAYS - 1) * 86_400_000)),
				// So a set dated in the future doesn't stay in the charts until then.
				before: day(new Date(today.getTime() + 86_400_000))
			};

	return {
		month: monthParam?.[0] ?? null,
		place: place && placeValue(place),
		query: { ...period, ...place }
	};
}
