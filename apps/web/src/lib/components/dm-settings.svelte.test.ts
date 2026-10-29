import { page } from 'vitest/browser';
import { describe, expect, it } from 'vitest';
import { render } from 'vitest-browser-svelte';
import type { ChannelView, DmAvailability } from '#lib/types.ts';
import DmSettings from './DmSettings.svelte';

const on: ChannelView = { status: 'active', pausedReason: null, address: null, topics: ['bookings'] };

const root = 'localnetwork.test';

const action = (name: string) => page.getByRole('button', { name }).element().closest('form')?.getAttribute('action');

describe('DmSettings', () => {
	it('shows a placeholder while Bluesky chat answers, then whether DMs can arrive', async () => {
		let answer: (state: DmAvailability) => void = () => {};

		const availability = new Promise<DmAvailability>((resolve) => (answer = resolve));

		await render(DmSettings, { available: true, channel: null, availability, root, topics: ['bookings'] });

		await expect.element(page.getByText('Checking Bluesky chat')).toBeInTheDocument();
		expect(action('Turn on DMs')).toBe('?/dmOn');
		answer('ok');
		await expect.element(page.getByText('Bluesky chat can deliver DMs from @localnetwork.test to you.')).toBeVisible();
		await expect.element(page.getByText('Checking Bluesky chat')).not.toBeInTheDocument();
	});

	it('asks someone whose chat only takes DMs from accounts they follow to follow the root', async () => {
		await render(DmSettings, {
			available: true,
			channel: { ...on, status: 'paused', pausedReason: 'dm_not_allowed' },
			availability: Promise.resolve<DmAvailability>('needs_follow'),
			root,
			topics: ['bookings']
		});

		await expect
			.element(page.getByText('Follow @localnetwork.test, then check again.', { exact: false }))
			.toBeVisible();
		await expect
			.element(page.getByRole('link', { name: '@localnetwork.test on Bluesky' }))
			.toHaveAttribute('href', 'https://bsky.app/profile/localnetwork.test');
		expect(action('Check again')).toBe('?/dmOn');
	});

	it('offers to try again when Bluesky chat is unreachable', async () => {
		await render(DmSettings, {
			available: true,
			channel: null,
			availability: Promise.resolve<DmAvailability>('unavailable'),
			root,
			topics: ['bookings']
		});

		await expect.element(page.getByText("We couldn't reach Bluesky chat.")).toBeVisible();
		await expect.element(page.getByRole('button', { name: 'Try again' })).toHaveAttribute('type', 'button');
	});

	it('lets someone with DMs on pick topics, send a test, or turn them off', async () => {
		await render(DmSettings, {
			available: true,
			channel: on,
			availability: Promise.resolve<DmAvailability>('ok'),
			root,
			topics: ['bookings']
		});

		await expect.element(page.getByText('On, from @localnetwork.test.')).toBeVisible();
		await expect.element(page.getByRole('group', { name: 'What to DM' }).getByRole('checkbox')).toBeChecked();
		expect([action('Save'), action('Send a test'), action('Turn off DMs')]).toEqual([
			'?/dmTopics',
			'?/dmTest',
			'?/dmOff'
		]);
	});

	it("says when the site doesn't send DMs", async () => {
		await render(DmSettings, { available: false, channel: null, availability: null, root, topics: ['bookings'] });

		await expect.element(page.getByText('Not set up on this site.')).toBeVisible();
		expect(page.getByRole('button').elements()).toHaveLength(0);
	});
});
