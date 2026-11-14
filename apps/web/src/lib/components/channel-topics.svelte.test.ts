import { page } from 'vitest/browser';
import { expect, it } from 'vitest';
import { render } from 'vitest-browser-svelte';
import type { ChannelView } from '#lib/types.ts';
import ChannelTopics from './ChannelTopics.svelte';

const on: ChannelView = { status: 'active', pausedReason: null, address: null, topics: ['bookings'] };

it('lays topics against channels, each column saving to its own action', async () => {
	await render(ChannelTopics, {
		topics: ['bookings', 'events'],
		channels: [
			{ channel: 'email', view: on },
			{ channel: 'dm', view: { ...on, status: 'paused', topics: [] } }
		]
	});

	await expect.element(page.getByRole('checkbox', { name: 'Bookings on the site, always' })).toBeDisabled();
	await expect.element(page.getByRole('checkbox', { name: 'Bookings by Email' })).toBeChecked();
	await expect.element(page.getByRole('checkbox', { name: 'Events by Email' })).not.toBeChecked();
	await expect.element(page.getByRole('checkbox', { name: 'Bookings by Bluesky DM' })).not.toBeChecked();
	await expect.element(page.getByRole('columnheader', { name: 'Bluesky DM Paused' })).toBeVisible();

	const formOf = (name: string) => {
		const box = page.getByRole('checkbox', { name }).element();

		return box instanceof HTMLInputElement ? box.form : null;
	};

	expect([
		formOf('Events by Email')?.getAttribute('action'),
		formOf('Events by Bluesky DM')?.getAttribute('action')
	]).toEqual(['?/emailTopics', '?/dmTopics']);
	expect(new FormData(formOf('Bookings by Email') ?? undefined).getAll('topic')).toEqual(['bookings']);
});

it("can't tick a channel that's off", async () => {
	await render(ChannelTopics, { topics: ['bookings'], channels: [{ channel: 'dm', view: null }] });

	await expect.element(page.getByRole('columnheader', { name: 'Bluesky DM Off' })).toBeVisible();
	await expect.element(page.getByRole('checkbox', { name: 'Bookings by Bluesky DM' })).toBeDisabled();
});
