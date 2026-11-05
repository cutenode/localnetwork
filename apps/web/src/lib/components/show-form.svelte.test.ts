import { page } from 'vitest/browser';
import { describe, expect, it } from 'vitest';
import { showFormDefaults } from '#lib/shows.ts';
import { posted, renderInForm } from '#lib/testing/forms.ts';
import ShowForm from './ShowForm.svelte';

const winterSocial = {
	...showFormDefaults,
	name: 'Winter Social',
	date: '2026-12-12',
	start: '22:00',
	city: 'Berlin',
	country: 'DE',
	showCid: 'bafyshow',
	eventCid: 'bafyevent'
};

const field = (name: string) => page.getByLabelText(name, { exact: true });

const cancelled = () => page.getByRole('checkbox', { name: 'This show is cancelled' });

describe('ShowForm', () => {
	it('posts what was entered, and starts over from new values', async () => {
		const { form, rerender } = await renderInForm(ShowForm, { values: winterSocial, geocoder: false, editing: true });

		await field('Show name').fill('Winter Social II');
		await field('Date').fill('2026-12-19');
		await field('Starts').fill('23:00');
		await field('Ends (optional)').fill('06:00');
		await field('Ticket link (optional)').fill('https://tickets.test/winter');
		await field('Venue (optional)').fill('Kantine');
		await cancelled().click();

		expect(posted(form, 'name', 'date', 'start', 'end', 'tickets', 'venue', 'cancelled')).toEqual({
			name: ['Winter Social II'],
			date: ['2026-12-19'],
			start: ['23:00'],
			end: ['06:00'],
			tickets: ['https://tickets.test/winter'],
			venue: ['Kantine'],
			cancelled: ['on']
		});

		await rerender({ values: { ...winterSocial, name: 'Spring Social' }, geocoder: false, editing: true });
		await expect.element(field('Show name')).toHaveValue('Spring Social');
		await expect.element(field('Venue (optional)')).toHaveValue('');
		await expect.element(cancelled()).not.toBeChecked();
	});

	it('posts the versions an edit swaps on', async () => {
		const { form } = await renderInForm(ShowForm, { values: winterSocial, geocoder: false, editing: true });

		expect(posted(form, 'showCid', 'eventCid')).toEqual({ showCid: ['bafyshow'], eventCid: ['bafyevent'] });
	});

	it('leaves the versions and cancelling out of a new show', async () => {
		const { form } = await renderInForm(ShowForm, { values: winterSocial, geocoder: false });

		expect(posted(form, 'showCid', 'eventCid', 'cancelled')).toEqual({ showCid: [], eventCid: [], cancelled: [] });
		await expect.element(cancelled()).not.toBeInTheDocument();
	});
});
