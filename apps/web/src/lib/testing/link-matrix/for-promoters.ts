import { render } from 'vitest-browser-svelte';
import Page from '../../../routes/for-promoters/+page.svelte';
import { NEON_TIDE, PAT, type LinkChecks } from '#lib/testing/link-matrix.ts';
import { PAGE_META } from '#lib/testing/page-meta.ts';

const shortlist = {
	genre: 'techno',
	place: null,
	href: '/match?go=1&genres=techno&radius=50',
	results: [
		{
			candidate: NEON_TIDE,
			score: 80,
			breakdown: { genre: 100, distance: 50, budget: 50, capacity: 50, setType: 50, format: 50 },
			reasons: ['Plays techno'],
			warnings: []
		}
	]
};

export default {
	pageType: 'For promoters',
	// Planner is the closing call for someone already promoting; every other row is there signed out.
	render: (row) =>
		render(Page, {
			params: {},
			data: {
				viewer: row === 'Planner' ? PAT : null,
				pendingRequests: 0,
				meta: PAGE_META,
				geocoder: false,
				directory: Promise.resolve({ example: NEON_TIDE, djs: 1, cities: 1, unclaimed: 0 }),
				shortlist: Promise.resolve(shortlist),
				promoting: Promise.resolve(row === 'Planner')
			},
			form: null
		}),
	links: {
		'Find DJs': [{ name: 'Open Find DJs', href: '/projects' }],
		'Acts and listed DJs': [
			{ name: /^Neon Tide/, href: '/projects/neon-tide', within: 'Search the way you book.' },
			{ name: 'Neon Tide', href: '/projects/neon-tide', within: 'Describe the night. Get a shortlist with reasons.' }
		],
		'Match a show': [{ name: 'Match a show', href: '/match?go=1&genres=techno&radius=50' }],
		'Press kits': [{ name: "See Neon Tide's press kits", href: '/projects/neon-tide/press-kits' }],
		'For DJs': [{ name: 'For DJs', href: '/for-djs' }],
		'Turning on promoting': [{ name: 'Sign in', href: '/login?next=%2Fdashboard%2Fprofile%23section-promoter' }],
		Planner: [{ name: 'Your planner', href: '/dashboard/planner' }]
	}
} satisfies LinkChecks;
