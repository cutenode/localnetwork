import { page } from 'vitest/browser';
import { expect, it } from 'vitest';
import { render } from 'vitest-browser-svelte';
import { sampleReport } from '#lib/mix/sample.ts';
import type { DjSetView, ProjectStatus } from '#lib/types.ts';
import Page from './+page.svelte';
import { PAGE_META } from '#lib/testing/page-meta.ts';

const set = { uri: 'at://did:plc:neon/computer.localnetwork.dj.set/3kset', cid: 'bafylinked' };

const view: DjSetView = { ...set, title: 'Closing', date: '2026-09-01', venue: 'Tresor', trackCount: 24 };

const show = (sets?: Promise<Map<string, DjSetView> | null>, status: ProjectStatus = 'active') =>
	render(Page, {
		params: { slug: 'neon-tide', n: '1' },
		data: {
			viewer: null,
			pendingRequests: 0,
			meta: PAGE_META,
			project: { slug: 'neon-tide', name: 'Neon Tide', avatarUrl: null, status },
			mix: { title: 'Tidal Hours', url: 'https://mixes.test/tidal', set },
			sets,
			source: Promise.resolve(null),
			n: 1,
			report: sampleReport
		},
		form: null
	});

it('shows the set the mix was recorded at, with the report', async () => {
	await show(Promise.resolve(new Map([[set.uri, { ...view, cid: 'bafyedited' }]])));
	await expect.element(page.getByRole('link', { name: 'Closing' })).toHaveAttribute('href', set.uri);
	await expect.element(page.getByText(/Changed since it was linked/)).toBeInTheDocument();
	// The page is titled with the profile's name for the mix, not the one in the exported report.
	await expect.element(page.getByRole('heading', { level: 1, name: 'Tidal Hours' })).toBeInTheDocument();
});

it('falls back to the link when the directory is down', async () => {
	await show(Promise.resolve(null));
	await expect.element(page.getByRole('link', { name: 'linked set' })).toHaveAttribute('href', set.uri);
});

it("tells managers and staff when visitors can't see the mix yet", async () => {
	await show(undefined, 'hidden');
	await expect.element(page.getByText('Not public yet.')).toBeInTheDocument();
});

it('says nothing about visibility on a public project', async () => {
	await show();
	await expect.element(page.getByRole('heading', { level: 1, name: 'Tidal Hours' })).toBeInTheDocument();
	await expect.element(page.getByText('Not public yet.')).not.toBeInTheDocument();
});
