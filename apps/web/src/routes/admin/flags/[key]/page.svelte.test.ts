import { page } from 'vitest/browser';
import { expect, it } from 'vitest';
import { render } from 'vitest-browser-svelte';
import { ada } from '#lib/testing/requests.ts';
import type { FlagView } from '#lib/types.ts';
import Page from './+page.svelte';

it('lists the kinds of page the flag shows on', async () => {
	await renderFlag({ showsOn: ['profile'] });

	await expect.element(page.getByText('Shows on: Profile', { exact: true })).toBeVisible();
});

it('names each kind as the staff console does, and every page for global', async () => {
	await renderFlag({ showsOn: ['pressKit', 'party'] });

	await expect.element(page.getByText('Shows on: Press kit, Party', { exact: true })).toBeVisible();

	await renderFlag({ showsOn: ['global'] });

	await expect.element(page.getByText('Shows on: Every page', { exact: true })).toBeVisible();
});

it('says so when the flag lists no kind of page', async () => {
	await renderFlag({ showsOn: [] });

	await expect.element(page.getByText('Shows on: None listed', { exact: true })).toBeVisible();
});

// Helpers

/** The flag page for parityCanary, with `fields` on top of a flag nobody has set. */
function renderFlag(fields: Partial<FlagView>) {
	const flag: FlagView = {
		key: 'parityCanary',
		description: 'Gates nothing.',
		showsOn: [],
		enabled: true,
		everyone: false,
		percentage: 0,
		groups: [],
		accounts: [],
		updatedAt: null,
		readyToRemove: false,
		...fields
	};

	return render(Page, {
		params: { key: flag.key },
		form: null,
		data: { viewer: ada, pendingRequests: 0, grants: ['flags'], flag, groups: [] }
	});
}
