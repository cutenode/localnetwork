import { page } from 'vitest/browser';
import { describe, expect, it } from 'vitest';
import { render } from 'vitest-browser-svelte';
import type { ChannelView } from '#lib/types.ts';
import EmailSettings from './EmailSettings.svelte';

const on: ChannelView = { status: 'active', pausedReason: null, address: 'neon@example.test', topics: ['bookings'] };

const action = (name: string) => page.getByRole('button', { name }).element().closest('form')?.getAttribute('action');

describe('EmailSettings', () => {
	it('offers to turn email on, saying when the last try was declined', async () => {
		const { rerender } = await render(EmailSettings, { available: true, channel: null, topics: ['bookings'] });

		expect(action('Turn on email')).toBe('?/emailOn');
		await expect.element(page.getByText("You didn't allow sharing")).not.toBeInTheDocument();
		await rerender({ available: true, channel: null, topics: ['bookings'], returned: 'declined' });
		await expect
			.element(page.getByText("You didn't allow sharing your email address, so email is still off."))
			.toBeVisible();
	});

	it('shows where email goes once on, with the topics to pick, a test and a way off', async () => {
		await render(EmailSettings, { available: true, channel: on, topics: ['bookings'] });

		await expect.element(page.getByText('On, sending to neon@example.test.')).toBeVisible();
		await expect
			.element(
				page
					.getByRole('group', { name: 'What to email' })
					.getByRole('checkbox', { name: 'Booking inquiries, messages and status changes' })
			)
			.toBeChecked();
		expect([action('Save'), action('Send a test'), action('Turn off email')]).toEqual([
			'?/emailTopics',
			'?/emailTest',
			'?/emailOff'
		]);
	});

	it('says why email is paused and what to do next', async () => {
		const { rerender } = await render(EmailSettings, {
			available: true,
			channel: { ...on, status: 'paused', pausedReason: 'email_unverified' },
			topics: ['bookings']
		});

		await expect
			.element(
				page.getByText('We emailed a link to neon@example.test. Open it within a day to start getting email there.')
			)
			.toBeVisible();
		expect(action('Send a new link')).toBe('?/emailOn');

		await rerender({
			available: true,
			channel: { ...on, status: 'paused', pausedReason: 'address_rejected' },
			topics: ['bookings']
		});
		await expect
			.element(page.getByText("Email to neon@example.test didn't go through.", { exact: false }))
			.toBeVisible();
		expect(action('Check again')).toBe('?/emailOn');

		await rerender({
			available: true,
			channel: { ...on, status: 'paused', pausedReason: 'email_unverified' },
			topics: ['bookings'],
			returned: 'limited'
		});
		await expect
			.element(page.getByText("We can't send a confirmation link to that address right now. Try again tomorrow."))
			.toBeVisible();
	});

	it("says when the site can't send email at all", async () => {
		await render(EmailSettings, { available: false, channel: null, topics: ['bookings'] });

		await expect.element(page.getByText('Not set up on this site.')).toBeVisible();
		expect(page.getByRole('button').elements()).toHaveLength(0);
	});
});
