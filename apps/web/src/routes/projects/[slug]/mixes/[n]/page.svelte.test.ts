import { page } from 'vitest/browser';
import { expect, it } from 'vitest';
import { render } from 'vitest-browser-svelte';
import { sampleReport } from '#lib/mix/sample.ts';
import type { DjSetView } from '#lib/types.ts';
import Page from './+page.svelte';

const set = { uri: 'at://did:plc:neon/computer.localnetwork.dj.set/3kset', cid: 'bafylinked' };

const view: DjSetView = { ...set, title: 'Closing', date: '2026-09-01', venue: 'Tresor', trackCount: 24 };

const show = (sets?: Promise<Map<string, DjSetView> | null>) =>
	render(Page, {
		params: { slug: 'neon-tide', n: '1' },
		data: {
			viewer: null,
			pendingRequests: 0,
			project: { slug: 'neon-tide', name: 'Neon Tide' },
			mix: { title: 'Tidal Hours', url: 'https://mixes.test/tidal', set },
			sets,
			n: 1,
			report: sampleReport
		},
		form: null
	});

it('shows the set the mix was recorded at, with the report', async () => {
	await show(Promise.resolve(new Map([[set.uri, { ...view, cid: 'bafyedited' }]])));
	await expect.element(page.getByRole('link', { name: 'Closing' })).toHaveAttribute('href', set.uri);
	await expect.element(page.getByText(/Changed since it was linked/)).toBeInTheDocument();
	await expect.element(page.getByRole('heading', { level: 1, name: 'Sunday Warm-up' })).toBeInTheDocument();
});

it('falls back to the link when the directory is down', async () => {
	await show(Promise.resolve(null));
	await expect.element(page.getByRole('link', { name: 'linked set' })).toHaveAttribute('href', set.uri);
});
