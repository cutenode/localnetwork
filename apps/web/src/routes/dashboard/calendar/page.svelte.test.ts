import { page } from 'vitest/browser';
import { expect, it } from 'vitest';
import { render } from 'vitest-browser-svelte';
import type { NightRow } from '#lib/home.ts';
import type { ProjectView, Viewer } from '#lib/types.ts';
import Page from './+page.svelte';
import type { PageProps } from './$types';

const viewer: Viewer = {
	did: 'did:plc:neon',
	handle: 'neon.test',
	roles: [],
	isAdmin: false,
	isStaff: false,
	spacesCapable: true
};

const night = (title: string, date: string, overrides: Partial<NightRow> = {}): NightRow => ({
	key: title,
	date,
	title,
	detail: 'Booking for Neon Tide · not confirmed',
	href: `/dashboard/bookings/${title}`,
	tentative: true,
	...overrides
});

const nights = (rows: NightRow[], failed = false) => Promise.resolve({ nights: rows, more: 0, failed });

// SAFETY: the page reads only an act's slug and status.
const act = { slug: 'neon-tide', status: 'active' } as ProjectView;

const base: PageProps['data'] = {
	viewer,
	pendingRequests: 0,
	unread: 0,
	sections: Promise.resolve({ performer: true, promoter: false }),
	projects: Promise.resolve([act]),
	waiting: 0,
	needs: [],
	access: Promise.resolve([]),
	nights: nights([])
};

const open = (data: Partial<PageProps['data']>) => render(Page, { params: {}, form: null, data: { ...base, ...data } });

it('groups nights by month, soonest first, and flags inquiries that clash', async () => {
	const year = new Date().getUTCFullYear() + 1;

	await open({
		nights: nights([
			night('Rooftop Rave', `${year}-01-14`, { inquiries: 2 }),
			night('Basement Late', `${year}-01-14`),
			night('Closing Night', `${year}-02-21`, { tentative: false, detail: 'You play 23:00 as Neon Tide' })
		])
	});

	const january = page.getByRole('region', { name: `January ${year}` });
	await expect
		.element(january.getByRole('link', { name: 'Rooftop Rave' }))
		.toHaveAttribute('href', '/dashboard/bookings/Rooftop Rave');
	await expect.element(january.getByText('2 inquiries on this night')).toBeInTheDocument();
	expect(january.getByRole('listitem').elements()).toHaveLength(2);
	await expect
		.element(page.getByRole('region', { name: `February ${year}` }).getByText('You play 23:00 as Neon Tide'))
		.toBeInTheDocument();
	expect(page.getByText(/inquiries on this night/).elements()).toHaveLength(1);
});

it('offers a DJ with an act a show to announce when nothing is coming up', async () => {
	await open({});

	await expect.element(page.getByText('No nights coming up')).toBeInTheDocument();
	await expect
		.element(page.getByRole('link', { name: 'Announce a show' }))
		.toHaveAttribute('href', '/dashboard/projects/neon-tide/shows/new');
	await expect.element(page.getByRole('link', { name: 'Plan a show' })).not.toBeInTheDocument();
});

it('offers a promoter a show to plan and DJs to find', async () => {
	await open({ sections: Promise.resolve({ performer: false, promoter: true }), projects: Promise.resolve([]) });

	await expect
		.element(page.getByRole('link', { name: 'Plan a show' }))
		.toHaveAttribute('href', '/dashboard/planner/shows/new');
	await expect.element(page.getByRole('link', { name: 'Find DJs' })).toHaveAttribute('href', '/projects');
	await expect.element(page.getByRole('link', { name: 'Announce a show' })).not.toBeInTheDocument();
});

it('says when some nights are missing instead of calling the calendar empty', async () => {
	await open({ nights: nights([], true) });

	await expect.element(page.getByText(/Some of your nights didn't load/)).toBeInTheDocument();
	await expect.element(page.getByRole('link', { name: 'Try again' })).toHaveAttribute('href', '/dashboard/calendar');
	await expect.element(page.getByText('No nights coming up')).not.toBeInTheDocument();
});

it('holds the list room while the nights load', async () => {
	await open({ nights: new Promise(() => {}) });

	await expect.element(page.getByRole('heading', { level: 1, name: 'Calendar' })).toBeInTheDocument();
	await expect.element(page.getByRole('status', { name: 'Loading your nights' })).toBeInTheDocument();
});
