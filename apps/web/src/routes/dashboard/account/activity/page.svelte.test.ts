import { page } from 'vitest/browser';
import { expect, it } from 'vitest';
import { render } from 'vitest-browser-svelte';
import { accountLayout, auditEntry } from '#lib/testing/account.ts';
import type { PageProps } from './$types';
import Page from './+page.svelte';

const entry = auditEntry(7);

const open = (activity: PageProps['data']['activity']) =>
	render(Page, { params: {}, form: null, data: { ...accountLayout, activity } });

it('lists entries as sentences, without internal words', async () => {
	await open({ entries: [entry], older: null });

	await expect.element(page.getByText('You followed Neon Tide')).toBeVisible();
	await expect.element(page.getByText(/hash-chained|repo|root copy/i)).not.toBeInTheDocument();
	await expect.element(page.getByRole('link', { name: 'Older' })).not.toBeInTheDocument();
});

it('links back further when there is more', async () => {
	await open({ entries: [entry], older: 7 });

	await expect.element(page.getByRole('link', { name: 'Older' })).toHaveAttribute('href', '?before=7');
});

it('says when there is nothing yet', async () => {
	await open({ entries: [], older: null });

	await expect.element(page.getByText('Nothing yet')).toBeVisible();
});

it('keeps the heading while the list loads', async () => {
	await open(new Promise(() => {}));

	await expect.element(page.getByRole('heading', { level: 1 })).toHaveTextContent('Activity');
	await expect.element(page.getByRole('status', { name: 'Loading your activity' })).toBeInTheDocument();
});

it('offers to try again when the list fails', async () => {
	const failed = Promise.reject(new Error('disk gone'));
	// Handled here as SvelteKit's streaming does, so the browser doesn't report it before the page reads it.
	failed.catch(() => {});
	await open(failed);

	await expect.element(page.getByText("Couldn't load your activity")).toBeVisible();
	await expect
		.element(page.getByRole('link', { name: 'Try again' }))
		.toHaveAttribute('href', '/dashboard/account/activity');
});
