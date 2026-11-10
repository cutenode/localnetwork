import { FEE_TIERS, FORMATS, labelFor, PROJECT_KINDS, SET_TYPES } from '#lib/taxonomy/options.ts';

/** The directory's filters as its page reads them from the address (routes/projects/+page.server.ts). */
export type DirectoryFilters = {
	q: string;
	place: string;
	lat: string;
	radiusKm: number;
	genres: string[];
	kinds: string[];
	formats: string[];
	setTypes: string[];
	feeTiers: string[];
	country: string;
	unclaimed: boolean;
};

/** One filter that's on, and the address of the same search without it. */
export type ActiveFilter = { label: string; href: string };

/** `search` without `name`, or without only `value` among a list's values. Paging starts over. */
function without(search: Pick<URLSearchParams, 'toString'>, name: string, value?: string): string {
	const params = new URLSearchParams(search.toString());
	params.delete('page');

	const kept =
		value === undefined
			? []
			: params
					.getAll(name)
					.flatMap((item) => item.split(','))
					.map((item) => item.trim())
					.filter((item) => item && item !== value);

	params.delete(name);

	for (const item of kept) params.append(name, item);

	// The coordinates belong to the place.
	if (name === 'place') ['lat', 'lng'].forEach((coordinate) => params.delete(coordinate));
	const query = params.toString();

	return query ? `/projects?${query}` : '/projects';
}

/** Every filter that's on, each removable on its own, in the order the form lists them. */
export function activeFilters(filters: DirectoryFilters, search: Pick<URLSearchParams, 'toString'>): ActiveFilter[] {
	const each = (name: string, values: string[], label: (value: string) => string = (value) => value) =>
		values.map((value) => ({ label: label(value), href: without(search, name, value) }));

	return [
		...(filters.q ? [{ label: `“${filters.q}”`, href: without(search, 'q') }] : []),
		...(filters.place && filters.lat
			? [{ label: `Within ${filters.radiusKm} km of ${filters.place}`, href: without(search, 'place') }]
			: []),
		...each('genres', filters.genres),
		...each('fee', filters.feeTiers, (value) => labelFor(FEE_TIERS, value)),
		...each('setType', filters.setTypes, (value) => labelFor(SET_TYPES, value)),
		...each('format', filters.formats, (value) => labelFor(FORMATS, value)),
		...each('kind', filters.kinds, (value) => labelFor(PROJECT_KINDS, value)),
		...(filters.country ? [{ label: filters.country, href: without(search, 'country') }] : []),
		...(filters.unclaimed ? [{ label: 'Unclaimed only', href: without(search, 'unclaimed') }] : [])
	];
}

/** What to try when a search finds nobody, going by the filters actually in use. */
export function emptyHint(filters: DirectoryFilters): string {
	const tries = [
		filters.place && filters.lat ? 'a wider radius' : '',
		filters.genres.length ? 'fewer genres' : '',
		filters.setTypes.length ? 'another slot' : '',
		filters.feeTiers.length ? 'another fee tier' : '',
		filters.formats.length || filters.kinds.length || filters.country || filters.unclaimed
			? 'fewer filters under More filters'
			: ''
	].filter(Boolean);

	if (tries.length)
		return `Try ${tries.length > 1 ? `${tries.slice(0, -1).join(', ')} or ${tries.at(-1)}` : tries[0]}.`;

	return filters.q ? 'Try a shorter search, or a genre from the list.' : 'Nobody is listed yet.';
}
