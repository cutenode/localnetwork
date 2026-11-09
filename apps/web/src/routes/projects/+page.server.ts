import { isValidLatLng } from '#lib/geo.ts';
import { clientIp } from '#lib/server/http.ts';
import { unlessOffline } from '#lib/server/happyview/offline.ts';
import { pageMeta } from '#lib/server/meta.ts';
import { FEE_TIERS, FORMATS, isOneOf, PROJECT_KINDS, SET_TYPES } from '#lib/taxonomy/options.ts';
import type { PageServerLoad } from './$types';

const list = (url: URL, name: string) => [
	...new Set(
		url.searchParams
			.getAll(name)
			.flatMap((value) => value.split(','))
			.map((value) => value.trim())
			.filter(Boolean)
			.slice(0, 20)
	)
];

export const load: PageServerLoad = async (event) => {
	const { url, locals } = event;
	const { performers, geocoder, limits } = locals.app;
	let lat = Number(url.searchParams.get('lat'));
	let lng = Number(url.searchParams.get('lng'));
	let hasNear = url.searchParams.get('lat') !== null && url.searchParams.get('lat') !== '' && isValidLatLng(lat, lng);
	const place = url.searchParams.get('place')?.trim().slice(0, 200) ?? '';

	// A place typed without picking it from the list has no coordinates. Look it up once, under the same limit as the
	// list's own lookups, so the distance filter still applies; when nothing is found the page says so instead.
	if (!hasNear && place && geocoder.enabled && limits.search.take(`places:${clientIp(event)}`)) {
		const [first] = await geocoder.search(place);

		if (first) {
			lat = first.latE6 / 1e6;
			lng = first.lngE6 / 1e6;
			hasNear = true;
		}
	}

	const radiusKm = Math.min(2000, Math.max(5, Number(url.searchParams.get('radius')) || 50));
	const sortParam = url.searchParams.get('sort');

	const sort =
		sortParam === 'distance' || sortParam === 'recent' || sortParam === 'name' || sortParam === 'relevance'
			? sortParam
			: undefined;

	const filters = {
		q: url.searchParams.get('q')?.slice(0, 100) ?? '',
		place,
		lat: hasNear ? String(lat) : '',
		lng: hasNear ? String(lng) : '',
		radiusKm,
		genres: list(url, 'genres'),
		kinds: list(url, 'kind').filter((value) => isOneOf(PROJECT_KINDS, value)),
		formats: list(url, 'format').filter((value) => isOneOf(FORMATS, value)),
		setTypes: list(url, 'setType').filter((value) => isOneOf(SET_TYPES, value)),
		feeTiers: list(url, 'fee').filter((value) => isOneOf(FEE_TIERS, value)),
		country: /^[A-Za-z]{2}$/.test(url.searchParams.get('country') ?? '')
			? url.searchParams.get('country')!.toUpperCase()
			: '',
		sort: sort ?? '',
		unclaimed: url.searchParams.has('unclaimed')
	};

	/** Whether a place was typed that couldn't be found, so no distance filter applies. */
	const placeMissed = !!place && !hasNear;

	const page = Math.max(1, Number(url.searchParams.get('page')) || 1);

	// Projects and the DJs staff list, together. Null when the directory is offline: the filters still render, so a retry
	// is one click.
	const result = await unlessOffline(locals.log, () =>
		performers.search({
			q: filters.q || undefined,
			near: hasNear ? { lat, lng } : undefined,
			radiusKm,
			genres: filters.genres,
			kinds: filters.kinds,
			formats: filters.formats,
			setTypes: filters.setTypes,
			feeTiers: filters.feeTiers,
			country: filters.country || undefined,
			unclaimed: filters.unclaimed,
			sort,
			page,
			pageSize: 24
		})
	);

	return {
		result,
		filters,
		placeMissed,
		geocoder: geocoder.enabled,
		meta: pageMeta(locals.app.config.publicUrl, {
			title: 'Find DJs',
			description: "Search Local Network's DJs by sound, place, fee and slot.",
			path: '/projects'
		})
	};
};
