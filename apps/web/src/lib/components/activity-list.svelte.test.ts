import { page } from 'vitest/browser';
import { expect, it } from 'vitest';
import { render } from 'vitest-browser-svelte';
import { formatDateTime } from '#lib/format.ts';
import ActivityList from './ActivityList.svelte';

const entry = {
	id: '1',
	action: 'follow.create',
	summary: 'Followed Neon Tide',
	outcome: 'success',
	createdAt: '2020-01-02T10:00:00Z',
	actorStatus: 'verified',
	rootStatus: 'unsupported'
} as const;

it('says what happened and when, with the exact time on hover', async () => {
	await render(ActivityList, { entries: [entry] });

	await expect.element(page.getByText('You followed Neon Tide')).toBeVisible();
	const time = page.getByText(/years ago/);
	await expect.element(time).toHaveAttribute('datetime', entry.createdAt);
	await expect.element(time).toHaveAttribute('title', formatDateTime(entry.createdAt));
});

it('keeps the technical detail behind Details', async () => {
	await render(ActivityList, { entries: [entry] });

	await expect.element(page.getByText('follow.create')).not.toBeVisible();
	await expect.element(page.getByText('Your copy')).not.toBeVisible();

	await page.getByText('Details').click();

	await expect.element(page.getByText('follow.create')).toBeVisible();
	await expect.element(page.getByText('Your copy')).toBeVisible();
	await expect.element(page.getByText('Saved and checked')).toBeVisible();
	await expect.element(page.getByText("Your account's server can't keep one")).toBeVisible();
});

it('holds the room of a page of them while loading', async () => {
	await render(ActivityList, { loading: true });

	await expect.element(page.getByRole('list')).toHaveAttribute('aria-busy', 'true');
	expect(page.getByRole('listitem').elements()).toHaveLength(6);
});
