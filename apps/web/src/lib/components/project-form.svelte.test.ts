import { createRawSnippet } from 'svelte';
import { page } from 'vitest/browser';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { render } from 'vitest-browser-svelte';
import { draftKey, hasDraft } from '#lib/form-draft.svelte.ts';
import { stubFormAction } from '#lib/testing/form-action.ts';
import type { ProjectFormValues } from '#lib/types.ts';
import ProjectForm from './ProjectForm.svelte';

const posted = (...names: string[]) => {
	const data = new FormData(document.querySelector('form')!);

	return Object.fromEntries(names.map((name) => [name, data.get(name)]));
};

const draft = draftKey({ did: 'did:plc:neon' }, 'project:new')!;

afterEach(() => {
	vi.unstubAllGlobals();
	localStorage.removeItem(draft);
});

describe('ProjectForm', () => {
	it('posts the private booking details and the note to moderators', async () => {
		await render(ProjectForm, { values: { kind: 'duo' }, showMessage: true, geocoder: false });

		await page.getByLabelText('Booking email').fill('book@neon.test');
		await page.getByLabelText('Booking phone').fill('+49 30 1234');
		await page.getByLabelText('Rates').fill('From 600 EUR');
		await page.getByLabelText('Tech rider').fill('2x CDJ-3000');
		await page.getByLabelText('Hospitality').fill('Water, a quiet room');
		await page.getByLabelText('Note to moderators (optional)').fill('We renamed last month.');

		expect(posted('kind', 'bookingEmail', 'bookingPhone', 'rateNotes', 'techRider', 'hospitality', 'message')).toEqual({
			kind: 'duo',
			bookingEmail: 'book@neon.test',
			bookingPhone: '+49 30 1234',
			rateNotes: 'From 600 EUR',
			techRider: '2x CDJ-3000',
			hospitality: 'Water, a quiet room',
			message: 'We renamed last month.'
		});
	});

	it('leaves the booking details out where they are asked for elsewhere', async () => {
		await render(ProjectForm, { showDetails: false, geocoder: false });

		await expect.element(page.getByRole('heading', { name: 'Private booking details' })).not.toBeInTheDocument();
		await expect.element(page.getByLabelText('Note to moderators (optional)')).not.toBeInTheDocument();
		expect(posted('bookingEmail', 'kind')).toEqual({ bookingEmail: null, kind: 'solo' });
	});

	it("keeps the draft when the project didn't go through", async () => {
		const server = stubFormAction();
		await render(ProjectForm, { values: { name: 'Neon Tide', city: 'Berlin', country: 'DE' }, draft, geocoder: false });

		await page.getByLabelText('Tagline').fill('Rolling techno');
		await expect.poll(() => hasDraft(draft)).toBe(true);
		await page.getByRole('button', { name: 'Submit for review' }).click();
		await expect.element(page.getByRole('button', { name: 'Saving…' })).toBeDisabled();
		server.answer();

		await expect.element(page.getByRole('button', { name: 'Submit for review' })).toBeEnabled();
		expect([server.sent[0]?.get('tagline'), hasDraft(draft)]).toEqual(['Rolling techno', true]);
	});

	it("adds the page's own fields, given the values the form shows", async () => {
		const children = createRawSnippet((fields: () => ProjectFormValues) => ({
			render: () => `<input type="hidden" name="claimFor" value="${fields().name}">`
		}));

		await render(ProjectForm, { values: { name: 'Neon Tide' }, geocoder: false, children });

		expect(posted('claimFor')).toEqual({ claimFor: 'Neon Tide' });
	});
});
