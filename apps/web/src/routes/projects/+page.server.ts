import { isValidLatLng } from '#lib/geo.ts';
import { unlessOffline } from '#lib/server/happyview/offline.ts';
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

export const load: PageServerLoad = async ({ url, locals }) => {
	const { projects, geocoder } = locals.app;
	const lat = Number(url.searchParams.get('lat'));
	const lng = Number(url.searchParams.get('lng'));
	const hasNear = url.searchParams.get('lat') !== null && url.searchParams.get('lat') !== '' && isValidLatLng(lat, lng);
	const radiusKm = Math.min(2000, Math.max(5, Number(url.searchParams.get('radius')) || 50));
	const sortParam = url.searchParams.get('sort');

	const sort =
		sortParam === 'distance' || sortParam === 'recent' || sortParam === 'name' || sortParam === 'relevance'
			? sortParam
			: undefined;

	const filters = {
		q: url.searchParams.get('q')?.slice(0, 100) ?? '',
		place: url.searchParams.get('place')?.slice(0, 200) ?? '',
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

	const page = Math.max(1, Number(url.searchParams.get('page')) || 1);

	// Null when the directory is offline: the filters still render, so a retry is one click.
	const result = await unlessOffline(locals.log, () =>
		projects.search({
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

	return { result, filters, geocoder: geocoder.enabled };
};
