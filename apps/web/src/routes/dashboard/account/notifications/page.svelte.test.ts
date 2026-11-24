import { page } from 'vitest/browser';
import { expect, it } from 'vitest';
import { render } from 'vitest-browser-svelte';
import { accountLayout } from '#lib/testing/account.ts';
import type { ChannelView } from '#lib/types.ts';
import type { PageProps } from './$types';
import Page from './+page.svelte';

const data = (available: boolean, channel: ChannelView | null = null): PageProps['data'] => ({
	...accountLayout,
	topics: ['bookings'],
	email: { available, notYet: false, channel, returned: null },
	dm: { available, notYet: false, channel, availability: Promise.resolve('ok'), root: 'localnetwork.computer' }
});

it('offers the grid while email or Bluesky DM can be set up', async () => {
	await render(Page, { params: {}, form: null, data: data(true) });

	await expect.element(page.getByRole('heading', { name: 'What goes where' })).toBeVisible();
	await expect.element(page.getByText('Pick what else goes by email or Bluesky DM.', { exact: false })).toBeVisible();
});

it('leaves the grid out when neither can be set up here', async () => {
	await render(Page, { params: {}, form: null, data: data(false) });

	await expect.element(page.getByRole('heading', { name: 'Notifications', level: 1 })).toBeVisible();
	await expect.element(page.getByRole('heading', { name: 'What goes where' })).not.toBeInTheDocument();
	await expect.element(page.getByText('Your inbox here gets everything.')).toBeVisible();
	await expect.element(page.getByText('Pick what else', { exact: false })).not.toBeInTheDocument();
});

it("says when email and DMs aren't on for their account yet", async () => {
	const off = data(false);

	await render(Page, {
		params: {},
		form: null,
		data: { ...off, email: { ...off.email, notYet: true }, dm: { ...off.dm, notYet: true } }
	});

	await expect.element(page.getByText('Not on for your account yet.').first()).toBeVisible();
	expect(page.getByText('Not on for your account yet.').elements()).toHaveLength(2);
});

it('lets someone turn off a channel they turned on before it stopped being offered to them', async () => {
	const on: ChannelView = { status: 'active', pausedReason: null, address: 'neon@example.test', topics: ['bookings'] };

	await render(Page, { params: {}, form: null, data: data(false, on) });

	await expect.element(page.getByText('Paused on this site for now.').first()).toBeVisible();
	await expect.element(page.getByRole('button', { name: 'Turn off email' })).toBeVisible();
	await expect.element(page.getByRole('button', { name: 'Turn off DMs' })).toBeVisible();
	await expect.element(page.getByRole('button', { name: 'Send a test' })).not.toBeInTheDocument();
});
