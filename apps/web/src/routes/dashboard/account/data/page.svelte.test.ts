import { page } from 'vitest/browser';
import { expect, it } from 'vitest';
import { render } from 'vitest-browser-svelte';
import { accountLayout } from '#lib/testing/account.ts';
import type { HeldRecordView } from '#lib/types.ts';
import type { PageProps } from './$types';
import Page from './+page.svelte';

const held = (kind: HeldRecordView['kind'], uri: string): HeldRecordView => ({
	uri,
	kind,
	title: 'A record',
	excerpt: null,
	createdAt: '2026-10-30T20:00:00Z'
});

const open = (records: PageProps['data']['records']) =>
	render(Page, { params: {}, form: null, data: { ...accountLayout, records } });

const row = (name: string) => page.getByRole('listitem').filter({ hasText: name });

it('counts what is held by type, with the records one step down', async () => {
	await open([held('booking', 'a'), held('booking', 'b'), held('request', 'c')]);

	await expect
		.element(page.getByRole('link', { name: 'Booking messages' }))
		.toHaveAttribute('href', '/dashboard/account/data/booking');
	await expect.element(row('Booking messages')).toHaveTextContent('Booking messages Inquiries and messages 2');
	await expect
		.element(page.getByRole('link', { name: 'Requests to staff' }))
		.toHaveAttribute('href', '/dashboard/account/data/request');
	await expect.element(row('Requests to staff')).toHaveTextContent("Requests to staff What you've asked staff for 1");
});

it('shows a type with nothing held as nothing, with nowhere to go', async () => {
	await open([held('booking', 'a')]);

	await expect
		.element(row('Press kit drafts'))
		.toHaveTextContent("Press kit drafts Changes you haven't published yet 0");
	await expect.element(page.getByRole('link', { name: 'Press kit drafts' })).not.toBeInTheDocument();
});

it('says what can and cannot be done, offering no download that does not exist', async () => {
	await open([]);

	await expect.element(page.getByRole('heading', { name: 'What you can do' })).toBeVisible();
	await expect.element(page.getByText('Delete', { exact: true })).toBeVisible();
	await expect.element(page.getByText(/download/i)).not.toBeInTheDocument();
});

it('keeps the room of every type while the counts load', async () => {
	await open(new Promise(() => {}));

	await expect.element(page.getByRole('status', { name: 'Loading your data' })).toBeInTheDocument();
	expect(page.getByRole('listitem').elements()).toHaveLength(4);
	await expect.element(page.getByRole('link', { name: 'Booking messages' })).not.toBeInTheDocument();
});

it('offers to try again when the counts fail', async () => {
	const failed = Promise.reject(new Error('directory down'));
	// Handled here as SvelteKit's streaming does, so the browser doesn't report it before the page reads it.
	failed.catch(() => {});
	await open(failed);

	await expect.element(page.getByText("Couldn't load your data")).toBeVisible();
	await expect
		.element(page.getByRole('link', { name: 'Try again' }))
		.toHaveAttribute('href', '/dashboard/account/data');
});
