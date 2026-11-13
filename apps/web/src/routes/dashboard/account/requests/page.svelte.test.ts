import { page } from 'vitest/browser';
import { expect, it } from 'vitest';
import { render } from 'vitest-browser-svelte';
import { accountLayout } from '#lib/testing/account.ts';
import { request } from '#lib/testing/requests.ts';
import type { PageProps } from './$types';
import Page from './+page.svelte';

const open = (requests: PageProps['data']['requests']) =>
	render(Page, { params: {}, form: null, data: { ...accountLayout, requests } });

it('says who has each request', async () => {
	await open([request('create')]);

	await expect.element(page.getByRole('heading', { level: 1 })).toHaveTextContent('Requests to staff');
	await expect.element(page.getByText(/With staff since .*\. Nothing needed from you\./)).toBeVisible();
});

it('holds their room while they load', async () => {
	await open(new Promise(() => {}));

	await expect.element(page.getByRole('status', { name: 'Loading your requests' })).toBeInTheDocument();
});

it('offers to try again when they fail', async () => {
	const failed = Promise.reject(new Error('disk gone'));
	// Handled here as SvelteKit's streaming does, so the browser doesn't report it before the page reads it.
	failed.catch(() => {});
	await open(failed);

	await expect.element(page.getByText("Couldn't load your requests")).toBeVisible();
	await expect
		.element(page.getByRole('link', { name: 'Try again' }))
		.toHaveAttribute('href', '/dashboard/account/requests');
});
