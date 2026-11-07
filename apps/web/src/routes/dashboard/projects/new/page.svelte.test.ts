import { page } from 'vitest/browser';
import { expect, it } from 'vitest';
import { render } from 'vitest-browser-svelte';
import type { DjSetView } from '#lib/types.ts';
import Page from './+page.svelte';

const closing: DjSetView = {
	uri: 'at://did:plc:dj/dj.set/closing',
	cid: 'bafyclosing',
	title: 'Closing',
	date: '2026-09-01',
	trackCount: 24
};

const renderForm = (sets: DjSetView[] | null) => {
	const data = {
		viewer: null,
		pendingRequests: 0,
		unread: 0,
		sections: Promise.resolve(null),
		projects: Promise.resolve(null),
		geocoder: false,
		sets: Promise.resolve(sets)
	};

	return render(Page, { params: {}, data, form: null });
};

it("offers the DJ's published sets on a mix", async () => {
	await renderForm([closing]);

	await expect.element(page.getByRole('option', { name: 'Closing · Sep 1, 2026' })).toBeInTheDocument();
});

it('says so when the DJ has no published sets', async () => {
	await renderForm([]);

	await expect.element(page.getByRole('option', { name: 'No published sets yet' })).toBeDisabled();
});
