import { page } from 'vitest/browser';
import { expect, it } from 'vitest';
import { render } from 'vitest-browser-svelte';
import { accountLayout } from '#lib/testing/account.ts';
import type { PageProps } from './$types';
import Page from './+page.svelte';

const data = (available: boolean): PageProps['data'] => ({
	...accountLayout,
	topics: ['bookings'],
	email: { available, channel: null, returned: null },
	dm: { available, channel: null, availability: Promise.resolve('ok'), root: 'localnetwork.computer' }
});

it('offers the grid while email or Bluesky DM can be set up', async () => {
	await render(Page, { params: {}, form: null, data: data(true) });

	await expect.element(page.getByRole('heading', { name: 'What goes where' })).toBeVisible();
});

it('leaves the grid out when neither can be set up here', async () => {
	await render(Page, { params: {}, form: null, data: data(false) });

	await expect.element(page.getByRole('heading', { name: 'Notifications', level: 1 })).toBeVisible();
	await expect.element(page.getByRole('heading', { name: 'What goes where' })).not.toBeInTheDocument();
});
