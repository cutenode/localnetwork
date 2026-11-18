import { page } from 'vitest/browser';
import { describe, expect, it } from 'vitest';
import { render } from 'vitest-browser-svelte';
import { formatShowDate, asShowDate } from '#lib/format.ts';
import WaitingEvents from './WaitingEvents.svelte';

const uri = 'at://did:plc:root/computer.localnetwork.space.party/3p/did:plc:root/computer.localnetwork.event/3e';

describe('WaitingEvents', () => {
	it("lists each event on the venue's clock, with its party, lineup and an Approve button", async () => {
		await render(WaitingEvents, {
			waiting: [{ uri, name: 'Winter Opening', startsAt: '2026-12-05T23:00:00+01:00', party: 'Night Shift', lineup: 2 }]
		});

		await expect.element(page.getByRole('heading', { name: 'Waiting for the venue' })).toBeVisible();
		await expect.element(page.getByText('Winter Opening')).toBeVisible();
		// On the venue's clock: 23:00 in Berlin, whatever zone the browser is in.
		const when = formatShowDate(asShowDate('2026-12-05T23:00:00+01:00'), true);
		expect(when).toMatch(/11:00|23:00/);
		await expect.element(page.getByText(`${when} · Night Shift · 2 performers on the lineup`)).toBeVisible();
		await expect.element(page.getByRole('button', { name: 'Approve Winter Opening' })).toBeVisible();
	});

	it('leaves out what it does not know, and says when nothing is waiting', async () => {
		const { rerender } = await render(WaitingEvents, {
			waiting: [{ uri, name: 'Closing', startsAt: '2026-12-31T22:00:00+01:00', party: null, lineup: 0 }]
		});

		await expect
			.element(page.getByText(formatShowDate(asShowDate('2026-12-31T22:00:00+01:00'), true), { exact: true }))
			.toBeVisible();

		await rerender({ waiting: [] });
		await expect.element(page.getByText(/^Nothing waiting/)).toBeVisible();
	});
});
