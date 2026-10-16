import { redirect } from '@sveltejs/kit';
import { isValidLatLng } from '#lib/geo.ts';
import { unlessOffline } from '#lib/server/happyview/offline.ts';
import { normalizeGenres } from '#lib/taxonomy/genres.ts';
import { FEE_TIERS, FORMATS, isOneOf, SET_TYPES } from '#lib/taxonomy/options.ts';
import type { ShowDetails } from '#lib/types.ts';
import { showDate } from '#lib/server/forms/show.ts';
import { act, all, requireViewer } from '#lib/server/http.ts';
import type { Actions, PageServerLoad } from './$types';

function criteriaFrom(params: URLSearchParams) {
	const lat = Number(params.get('lat'));
	const lng = Number(params.get('lng'));
	const hasLocation = !!params.get('lat') && isValidLatLng(lat, lng);
	const budget = params.get('budget') ?? '';
	const setType = params.get('setType') ?? '';
	const capacity = Number(params.get('capacity'));

	return {
		title: params.get('title')?.slice(0, 120) ?? '',
		date: params.get('date')?.slice(0, 10) ?? '',
		place: params.get('place')?.slice(0, 200) ?? '',
		lat: hasLocation ? String(lat) : '',
		lng: hasLocation ? String(lng) : '',
		location: hasLocation ? { lat, lng } : undefined,
		radiusKm: Math.min(2000, Math.max(5, Number(params.get('radius')) || 50)),
		genres: normalizeGenres(params.getAll('genres').flatMap((value) => value.split(','))),
		budgetTier: isOneOf(FEE_TIERS, budget) ? budget : undefined,
		capacity: Number.isInteger(capacity) && capacity > 0 ? capacity : undefined,
		setType: isOneOf(SET_TYPES, setType) ? setType : undefined,
		formats: [...new Set(params.getAll('format').filter((value) => isOneOf(FORMATS, value)))]
	};
}

export const load: PageServerLoad = async ({ url, locals }) => {
	const criteria = criteriaFrom(url.searchParams);
	const searched = url.searchParams.has('go');
	// Null when the directory is offline: the form keeps what was entered, so a retry is one click.
	const results = searched ? await unlessOffline(locals.log, () => locals.app.projects.match(criteria, 30)) : [];

	return { criteria, searched, results, geocoder: locals.app.geocoder.enabled };
};

export const actions: Actions = {
	saveShow: async ({ request, locals, url }) => {
		const viewer = requireViewer(locals, url);
		const form = await request.formData();
		// The action URL carries the search, so a failed save re-renders the same results.
		const criteria = criteriaFrom(url.searchParams);
		const lineup = all(form, 'project', 40).map((project) => ({ project, status: 'considering' as const }));

		const result = await act(async () => {
			const details: ShowDetails = { title: criteria.title || `Show${criteria.date ? ` on ${criteria.date}` : ''}` };

			if (criteria.date) details.date = showDate(criteria.date);

			if (criteria.place) details.venue = criteria.place;

			if (criteria.capacity) details.capacity = criteria.capacity;

			if (criteria.budgetTier) details.budgetTier = criteria.budgetTier;

			if (criteria.setType) details.setType = criteria.setType;

			if (criteria.genres.length) details.genres = criteria.genres;

			const rkey = await locals.app.planner.saveShow(viewer, { details, lineup });

			return { rkey };
		});

		if ('rkey' in result) redirect(303, `/planner/shows/${result.rkey}`);

		return result;
	}
};
