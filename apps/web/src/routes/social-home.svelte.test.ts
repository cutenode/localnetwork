import { page } from 'vitest/browser';
import { describe, expect, it } from 'vitest';
import { render } from 'vitest-browser-svelte';
import type { HomeArea } from '#lib/home-events.ts';
import { PAGE_META } from '#lib/testing/page-meta.ts';
import type { ProjectView, ShowView, SupportedTrack, Viewer } from '#lib/types.ts';
import type { PageData } from './$types';
import Page from './+page.svelte';

const act = (slug: string, name: string, mixes?: ProjectView['profile']['mixes']): ProjectView => ({
	uri: `at://did:plc:root/computer.localnetwork.project/${slug}`,
	rkey: slug,
	cid: 'c',
	slug,
	profile: { name, kind: 'solo', genres: ['techno'], location: { city: 'Berlin', country: 'DE' }, mixes },
	managers: [],
	status: 'active',
	revision: 1,
	createdBy: 'did:plc:a',
	createdAt: '2026-01-01T00:00:00Z',
	updatedAt: '2026-01-01T00:00:00Z',
	avatarUrl: null
});

/** Neon Tide has a photo; the others show their initials. */
const neon: ProjectView = {
	...act('neon-tide', 'Neon Tide', [
		{
			title: 'Tidal Hours 004',
			url: 'https://soundcloud.com/neon/tidal-hours',
			tracklist: { $type: 'blob', ref: { $link: 'bafy' }, mimeType: 'application/json', size: 2 }
		}
	]),
	avatarUrl: '/media/did:plc:neon/bafyavatar'
};

const halflight = act('halflight', 'Halflight', [{ title: 'Low Light', url: 'https://soundcloud.com/halflight/low' }]);

const show = (n: number, project: ProjectView, event: string, name: string): ShowView => ({
	uri: `at://did:plc:neon/computer.localnetwork.show/${n}`,
	cid: 'c',
	did: 'did:plc:neon',
	rkey: String(n),
	project,
	record: {
		project: project.uri,
		event: { uri: `at://did:plc:neon/community.lexicon.calendar.event/${event}`, cid: 'e' },
		name,
		startsAt: '2026-11-13T23:00:00+01:00',
		venue: 'Halle Nord',
		location: { city: 'Berlin', country: 'DE' },
		tickets: 'https://tickets.example/tidal',
		createdAt: '2026-11-01T00:00:00Z'
	}
});

/** Tidal Hours with two acts on its lineup, and Low Light with one. */
const tidal = [show(1, neon, 'tidal', 'Tidal Hours'), show(2, halflight, 'tidal', 'Tidal Hours')];

const low = show(3, halflight, 'low', 'Low Light');

const berlin = { place: 'Berlin', lat: 52.52, lng: 13.405, radiusKm: 50 };

const area: HomeArea = {
	place: berlin,
	shows: [...tidal, low],
	djs: [
		{ project: neon, follow: { followers: 12, following: false } },
		{ project: halflight, follow: null }
	]
};

const tracks: SupportedTrack[] = [{ id: '1', title: 'Night Bus', artist: 'Ferrule', artists: [], supports: 9 }];

const fan: Viewer = {
	did: 'did:plc:fan',
	handle: 'fan.test',
	roles: [],
	isAdmin: false,
	isStaff: false,
	spacesCapable: true
};

const home = (data: Partial<Extract<PageData, { social: true }>> = {}) =>
	render(Page, {
		params: {},
		form: null,
		data: {
			social: true,
			viewer: null,
			pendingRequests: 0,
			meta: PAGE_META,
			geocoder: false,
			picked: berlin,
			network: null,
			area: Promise.resolve(area),
			tracks: Promise.resolve(tracks),
			...data
		}
	});

describe('/ with the social home', () => {
	it("leads a visitor with what's on near the place they picked, and who's playing there", async () => {
		await home();

		await expect
			.element(page.getByRole('heading', { level: 1 }))
			.toHaveAccessibleName("What's on near you, and who's going.");
		await expect.element(page.getByRole('combobox', { name: 'Near' })).toHaveValue('Berlin');
		await expect.element(page.getByRole('heading', { name: 'Coming up near Berlin' })).toBeInTheDocument();
		const lineup = page.getByRole('list', { name: 'Lineup' }).first();
		await expect
			.element(lineup.getByRole('link', { name: 'Halflight' }))
			.toHaveAttribute('href', '/projects/halflight');
		await expect
			.element(page.getByRole('link', { name: 'Low Light', exact: true }))
			.toHaveAttribute('href', '/projects/halflight');
		await expect
			.element(page.getByRole('link', { name: 'Tickets' }).first())
			.toHaveAttribute('href', 'https://tickets.example/tidal');

		const djs = page.getByRole('region', { name: "Who's playing near Berlin" });
		await expect
			.element(djs.getByRole('link', { name: /Follow 12/ }))
			.toHaveAttribute('href', '/login?next=%2Fprojects%2Fneon-tide%23follow');
		await expect
			.element(page.getByRole('link', { name: /Tidal Hours 004/ }))
			.toHaveAttribute('href', '/projects/neon-tide/mixes/1');
		await expect
			.element(page.getByRole('link', { name: /Low Light Halflight/ }))
			.toHaveAttribute('href', '/projects/halflight');
		await expect
			.element(page.getByRole('list', { name: 'Most played this month' }).getByText('Night Bus'))
			.toBeVisible();
	});

	it('says what an account gets, and links the landing pages for DJs and promoters', async () => {
		await home();

		await expect.element(page.getByRole('link', { name: 'Sign in' }).last()).toHaveAttribute('href', '/login?next=%2F');
		await expect.element(page.getByRole('link', { name: 'Local Network for DJs' })).toHaveAttribute('href', '/for-djs');
		await expect
			.element(page.getByRole('link', { name: 'Local Network for promoters' }))
			.toHaveAttribute('href', '/for-promoters');
	});

	it('asks a first visitor to pick a city, and leaves out the sections that need one', async () => {
		await home({ picked: null, area: Promise.resolve(null) });

		await expect.element(page.getByText('Pick your city')).toBeVisible();
		await expect.element(page.getByRole('combobox', { name: 'Near' })).toHaveValue('');
		await expect.element(page.getByRole('region', { name: /Who's playing/ })).not.toBeInTheDocument();
	});

	it('offers a wider search when nothing is announced nearby', async () => {
		await home({ area: Promise.resolve({ ...area, shows: [], djs: [] }) });

		await expect
			.element(page.getByRole('link', { name: 'Look further out' }))
			.toHaveAttribute('href', '/shows?place=Berlin&lat=52.52&lng=13.405&radius=250');
	});

	it('holds places while it loads, and says when the directory is offline', async () => {
		await home({ area: new Promise(() => {}), tracks: new Promise(() => {}) });
		await expect.element(page.getByRole('list', { name: 'Loading shows' })).toBeInTheDocument();

		await home({ area: Promise.resolve({ ...area, shows: null, djs: null }), tracks: Promise.resolve(null) });
		await expect.element(page.getByText('Shows are offline')).toBeVisible();
		await expect.element(page.getByText("The charts aren't answering. Try again in a minute.")).toBeVisible();
	});

	it("leads a viewer with their network's shows, then what's near them, with Follow buttons for its DJs", async () => {
		await home({ viewer: fan, network: Promise.resolve(tidal), area: Promise.resolve({ ...area, shows: [low] }) });

		await expect.element(page.getByRole('heading', { level: 1 })).toHaveAccessibleName('Your network');
		await expect.element(page.getByText('and 1 other play Fri 13 Nov')).toBeVisible();
		await expect.element(page.getByRole('heading', { name: 'Near Berlin' })).toBeVisible();
		await expect
			.element(page.getByRole('link', { name: 'Low Light', exact: true }))
			.toHaveAttribute('href', '/projects/halflight');
		const follow = page.getByRole('button', { name: /Follow 12/ });
		await expect.element(follow).toBeEnabled();
		expect(follow.element().closest('form')?.getAttribute('action')).toBe('/projects/neon-tide?/follow');
	});

	it("says how to fill in a new account's home", async () => {
		await home({ viewer: fan, network: Promise.resolve([]), area: Promise.resolve(null) });

		await expect.element(page.getByRole('link', { name: 'Find DJs' }).last()).toHaveAttribute('href', '/projects');
		await expect.element(page.getByText('Follow DJs and acts to see their shows here')).toBeVisible();
		await expect
			.element(page.getByRole('link', { name: 'Set your home city' }))
			.toHaveAttribute('href', '/dashboard/profile');
	});

	it('marks cancelled and moved events, and leaves out mixes for acts without one', async () => {
		const cancelled = {
			...low,
			uri: 'cancelled',
			record: { ...low.record, status: 'community.lexicon.calendar.event#cancelled' }
		};

		const moved = {
			...tidal[0],
			record: { ...tidal[0].record, status: 'community.lexicon.calendar.event#rescheduled' }
		};

		const silent = act('silent', 'Silent Act');

		await home({
			area: Promise.resolve({
				...area,
				shows: [moved, cancelled],
				djs: [silent, halflight].map((project) => ({ project, follow: null }))
			})
		});

		await expect.element(page.getByText('Cancelled')).toBeVisible();
		await expect.element(page.getByText('New date')).toBeVisible();
		await expect
			.element(page.getByRole('link', { name: 'Tickets' }))
			.toHaveAttribute('href', 'https://tickets.example/tidal');
		await expect
			.element(page.getByRole('link', { name: /^Low Light\s+Halflight$/ }))
			.toHaveAttribute('href', '/projects/halflight');
		await expect.element(page.getByRole('link', { name: /^Silent Act\s+Silent Act$/ })).not.toBeInTheDocument();
	});

	it("says when a viewer's shows are offline, and when nothing else is near", async () => {
		await home({ viewer: fan, network: Promise.resolve(null), area: Promise.resolve({ ...area, shows: [], djs: [] }) });
		await expect.element(page.getByText('Shows are offline')).toBeVisible();
		await expect.element(page.getByText('Nothing else announced near Berlin yet.')).toBeVisible();
	});

	it("says when what's near a viewer is offline, under one act's show", async () => {
		await home({
			viewer: fan,
			network: Promise.resolve([low]),
			area: Promise.resolve({ ...area, shows: null, djs: null })
		});
		await expect.element(page.getByText('plays Fri 13 Nov')).toBeVisible();
		await expect.element(page.getByText('Shows are offline')).toBeVisible();
	});
});
