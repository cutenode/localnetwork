import { page } from 'vitest/browser';
import { afterEach, describe, expect, it } from 'vitest';
import { render } from 'vitest-browser-svelte';
import { draftKey, hasDraft } from '#lib/form-draft.svelte.ts';
import ProjectForm from './ProjectForm.svelte';

const KEY = draftKey({ did: 'did:plc:neon' }, 'project:new')!;

const values = {
	name: 'Neon Tide',
	links: [
		{ kind: 'soundcloud', url: 'https://soundcloud.test/neon' },
		{ kind: 'website', url: 'https://neon.test' }
	]
};

const stored = () => JSON.parse(localStorage.getItem(KEY) ?? 'null');

const notice = () => page.getByRole('status').filter({ hasText: 'unsent changes' });

afterEach(() => localStorage.removeItem(KEY));

describe('ProjectForm drafts', () => {
	it('keeps unsent changes, rows removed included, and brings them back on the next visit', async () => {
		const first = await render(ProjectForm, { values, draft: KEY, geocoder: false });

		await page.getByLabelText('Tagline').fill('Rolling techno');
		await page.getByRole('button', { name: 'Remove link' }).last().click();
		await expect.poll(() => stored()?.values).toMatchObject({ tagline: 'Rolling techno', links: [values.links[0]] });
		await first.unmount();

		await render(ProjectForm, { values, draft: KEY, geocoder: false });
		await expect.element(page.getByLabelText('Tagline')).toHaveValue('Rolling techno');
		expect(document.querySelectorAll('input[name=linkUrl]')).toHaveLength(1);
		await expect.element(notice().getByText(/Pick any files again/)).toBeInTheDocument();
		await expect.element(notice().getByText(/changed since/)).not.toBeInTheDocument();
	});

	it('drops the draft on discard, and once the fields are back to what they were', async () => {
		localStorage.setItem(
			KEY,
			JSON.stringify({
				savedAt: new Date().toISOString(),
				base: 'something older',
				values: { ...values, tagline: 'Old idea' }
			})
		);
		await render(ProjectForm, { values, draft: KEY, geocoder: false });
		await expect.element(page.getByLabelText('Tagline')).toHaveValue('Old idea');
		await expect.element(notice().getByText(/The saved version changed since/)).toBeInTheDocument();

		await page.getByRole('button', { name: 'Discard' }).click();
		await expect.element(page.getByLabelText('Tagline')).toHaveValue('');
		await expect.element(notice()).not.toBeInTheDocument();
		expect(hasDraft(KEY)).toBe(false);
		expect(document.querySelectorAll('input[name=linkUrl]')).toHaveLength(2);

		await page.getByLabelText('Tagline').fill('Second thoughts');
		await expect.poll(() => hasDraft(KEY)).toBe(true);
		await page.getByLabelText('Tagline').fill('');
		await expect.poll(() => hasDraft(KEY)).toBe(false);
	});

	it('keeps nothing without a key', async () => {
		await render(ProjectForm, { values, geocoder: false });
		await page.getByLabelText('Tagline').fill('Rolling techno');
		await expect.element(page.getByLabelText('Tagline')).toHaveValue('Rolling techno');
		expect(Object.keys(localStorage).filter((key) => key.startsWith('ln:draft:'))).toEqual([]);
	});
});
