import { page } from 'vitest/browser';
import { expect, it } from 'vitest';
import { render } from 'vitest-browser-svelte';
import { accountLayout } from '#lib/testing/account.ts';
import { inboxRow } from '#lib/testing/inbox.ts';
import type { PageProps } from './$types';
import Page from './+page.svelte';

const data: PageProps['data'] = {
	...accountLayout,
	unread: 1,
	inbox: { rows: [inboxRow('a')], older: undefined },
	paused: [],
	alsoSent: []
};

const open = (changes: Partial<PageProps['data']> = {}) =>
	render(Page, { params: {}, form: null, data: { ...data, ...changes } });

const SETTINGS = '/dashboard/account/notifications';

it('lists what happened, with a way to mark it all read and to get it elsewhere', async () => {
	await open();

	await expect.element(page.getByRole('heading', { level: 1, name: 'Inbox' })).toBeVisible();
	await expect.element(page.getByRole('button', { name: 'Mark all read' })).toBeVisible();
	await expect.element(page.getByRole('button', { name: /^Rooftop Rave/ })).toBeVisible();
	await expect
		.element(page.getByRole('link', { name: 'Get these by email or Bluesky DM' }))
		.toHaveAttribute('href', SETTINGS);
});

it('says where else these go only for channels that send', async () => {
	await open({ alsoSent: ['email', 'dm'] });

	await expect.element(page.getByText('Also sent to your email and as Bluesky DMs.', { exact: false })).toBeVisible();
	await expect.element(page.getByRole('link', { name: 'Change where these go' })).toHaveAttribute('href', SETTINGS);
});

it('puts a paused channel in one line pointing at the settings', async () => {
	await open({ paused: ['dm_not_allowed'] });

	await expect
		.element(page.getByText('Bluesky DMs are paused until your chat settings let ours through.', { exact: false }))
		.toBeVisible();
	await expect.element(page.getByRole('link', { name: 'Notification settings' })).toHaveAttribute('href', SETTINGS);

	await open({ paused: ['email_unverified', 'dm_not_allowed'] });
	await expect.element(page.getByText('Email and Bluesky DMs are paused.', { exact: false })).toBeVisible();
});

it('says when there is nothing new, with nothing to mark read', async () => {
	await open({ unread: 0, inbox: { rows: [], older: undefined } });

	await expect.element(page.getByText('Nothing new')).toBeVisible();
	expect(page.getByRole('button').elements()).toHaveLength(0);
});

it('holds the list while it loads and offers a retry when it fails', async () => {
	await open({ inbox: new Promise<never>(() => {}) });
	await expect.element(page.getByRole('status', { name: 'Loading your inbox' })).toBeVisible();

	await open({ inbox: Promise.reject(new Error('disk gone')) });
	await expect.element(page.getByText("Your inbox didn't load.", { exact: false })).toBeVisible();
	await expect.element(page.getByRole('link', { name: 'Try again' })).toHaveAttribute('href', '/dashboard/inbox');
});

it('links to older rows when there are more', async () => {
	await open({ inbox: { rows: [inboxRow('a')], older: 'a' } });

	await expect.element(page.getByRole('link', { name: 'Older' })).toHaveAttribute('href', '?before=a');
});
