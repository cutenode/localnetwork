import { page, userEvent } from 'vitest/browser';
import { describe, expect, it } from 'vitest';
import { render } from 'vitest-browser-svelte';
import headline from '#lib/headline.json' with { type: 'json' };
import type { ProjectView, ShowView, SupportedTrack } from '#lib/types.ts';
import type { PageData } from './$types';
import Page from './+page.svelte';
import { PAGE_META } from '#lib/testing/page-meta.ts';

const project: ProjectView = {
	uri: 'at://did:plc:root/computer.localnetwork.project/1',
	rkey: '1',
	cid: 'c',
	slug: 'neon-tide',
	profile: { name: 'Neon Tide', kind: 'solo', genres: ['techno'], location: { city: 'Berlin', country: 'DE' } },
	managers: [],
	status: 'active',
	revision: 1,
	createdBy: 'did:plc:a',
	createdAt: '2026-01-01T00:00:00Z',
	updatedAt: '2026-01-01T00:00:00Z',
	avatarUrl: null
};

const base = {
	social: false as const,
	shows: Promise.resolve([]),
	tracks: Promise.resolve([]),
	viewer: null,
	pendingRequests: 0,
	meta: PAGE_META,
	geocoder: false,
	tools: [
		{
			id: 'localthing',
			name: 'localthing',
			kind: 'Chrome extension',
			tagline: 'Does a thing.',
			lines: ['Keeps drives in sync']
		}
	]
};

const directory = {
	recent: [project],
	stats: { active: 12, cities: 10, unclaimed: 3 },
	genres: [{ value: 'techno', count: 2 }],
	cities: [{ city: 'Berlin', country: 'DE', count: 2 }]
};

const tidal: ShowView = {
	uri: 'at://did:plc:neon/computer.localnetwork.show/1',
	cid: 'c',
	did: 'did:plc:neon',
	rkey: '1',
	project,
	record: {
		project: project.uri,
		event: { uri: 'at://did:plc:neon/community.lexicon.calendar.event/1', cid: 'e' },
		name: 'Tidal Hours',
		startsAt: '2026-11-13T23:00:00+01:00',
		venue: 'Halle Nord',
		location: { city: 'Berlin', country: 'DE' },
		createdAt: '2026-11-01T00:00:00Z'
	}
};

const tracks: SupportedTrack[] = [{ id: '1', title: 'Night Bus', artist: 'Ferrule', artists: [], supports: 9 }];

const show = (data: PageData) => render(Page, { params: {}, form: null, data });

describe('/', () => {
	it('searches the directory, and lists new projects, sounds, cities and tools', async () => {
		await show({ ...base, ...directory });
		await expect
			.element(page.getByRole('heading', { level: 1 }))
			.toHaveAccessibleName(`${headline.verbs[0]} your ${headline.nouns[0]}.`);
		await expect.element(page.getByRole('search', { name: 'Find DJs' })).toHaveAttribute('action', '/projects');
		await expect.element(page.getByRole('textbox', { name: 'Sound or name' })).toHaveAttribute('name', 'q');
		await expect.element(page.getByRole('link', { name: 'Neon Tide' })).toHaveAttribute('href', '/projects/neon-tide');
		await expect
			.element(page.getByRole('link', { name: '3 up for claiming' }))
			.toHaveAttribute('href', '/projects?unclaimed=on');
		await expect
			.element(page.getByRole('link', { name: 'techno 2' }))
			.toHaveAttribute('href', '/projects?genres=techno');
		await expect
			.element(page.getByRole('link', { name: 'Berlin, DE 2' }))
			.toHaveAttribute('href', '/projects?q=Berlin&country=DE');
		await expect.element(page.getByRole('link', { name: /localthing/ })).toHaveAttribute('href', '/tools/localthing');
	});

	it('lists the next shows and the most played tracks', async () => {
		await show({ ...base, ...directory, shows: Promise.resolve([tidal]), tracks: Promise.resolve(tracks) });

		await expect.element(page.getByText('Tidal Hours')).toBeVisible();
		await expect.element(page.getByRole('link', { name: 'All shows' })).toHaveAttribute('href', '/shows');
		const played = page.getByRole('list', { name: 'Most played this month' });
		await expect.element(played.getByText('Night Bus')).toBeVisible();
		await expect.element(played.getByText('9 DJs')).toBeVisible();
		await expect.element(page.getByRole('link', { name: 'Charts' })).toHaveAttribute('href', '/charts');
	});

	it('holds a place for shows and tracks while they load', async () => {
		await show({ ...base, ...directory, shows: new Promise<null>(() => {}), tracks: new Promise<null>(() => {}) });

		await expect.element(page.getByRole('list', { name: 'Loading shows' })).toBeInTheDocument();
		expect(page.getByRole('list', { name: 'Most played this month' }).element().children).toHaveLength(5);
	});

	it('says when shows and charts are offline or empty', async () => {
		await show({ ...base, ...directory, shows: Promise.resolve(null), tracks: Promise.resolve(null) });
		await expect.element(page.getByText('Shows are offline')).toBeVisible();
		await expect.element(page.getByText("The charts aren't answering. Try again in a minute.")).toBeVisible();
	});

	it('says when nothing is announced or played yet', async () => {
		await show({ ...base, ...directory });
		await expect.element(page.getByText('No shows announced yet')).toBeVisible();
		await expect.element(page.getByText('Nothing played this month yet.')).toBeVisible();
	});

	it('marks what needs an account until someone signs in', async () => {
		const { rerender } = await show({ ...base, ...directory });
		await expect.element(page.getByRole('link', { name: /Private inquiries.*Sign in/ })).toBeVisible();
		expect(page.getByRole('link', { name: /Match a show/ }).element().textContent).not.toMatch(/Sign in/);

		await rerender({
			data: {
				...base,
				...directory,
				viewer: {
					did: 'did:plc:neon',
					handle: 'neon.test',
					roles: [],
					isAdmin: false,
					isStaff: false,
					spacesCapable: true
				}
			}
		});
		await expect
			.element(
				page.getByRole('link', {
					name: 'Private inquiries Each booking is its own thread, shared only with the people who answer for the DJ.'
				})
			)
			.toBeVisible();
	});

	it('holds the headline still while someone types in the search', async () => {
		await show({ ...base, ...directory });
		const heading = page.getByRole('heading', { level: 1 });

		await userEvent.click(page.getByRole('textbox', { name: 'Sound or name' }));
		const held = heading.element().textContent;
		await new Promise((resolve) => setTimeout(resolve, 3000));

		expect(heading.element().textContent).toBe(held);
	});

	it('says so when nothing is listed yet', async () => {
		await show({ ...base, recent: [], stats: { active: 0, cities: 0, unclaimed: 0 }, genres: [], cities: [] });
		await expect.element(page.getByText('No DJs listed yet.')).toBeInTheDocument();
		await expect.element(page.getByRole('heading', { name: 'Find DJs by sound or city' })).not.toBeInTheDocument();
	});

	it('keeps the page up when the directory is down', async () => {
		await show({ ...base, recent: [], stats: null, genres: [], cities: [] });
		await expect.element(page.getByText("The directory isn't answering right now.")).toBeInTheDocument();
		await expect.element(page.getByRole('search', { name: 'Find DJs' })).toBeInTheDocument();
	});
});
