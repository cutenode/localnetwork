import { page } from 'vitest/browser';
import { tick } from 'svelte';
import { afterEach, describe, expect, it } from 'vitest';
import { render } from 'vitest-browser-svelte';
import { draftKey, FormDraft, hasDraft } from '#lib/form-draft.svelte.ts';
import { clearSignedOutDrafts, SIGNED_OUT_COOKIE } from '#lib/signed-out-drafts.ts';
import DraftNotice from './DraftNotice.svelte';
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

describe('draftKey', () => {
	it('keeps no draft for someone signed out', () => {
		expect(draftKey(null, 'project:new')).toBeUndefined();
	});
});

describe('FormDraft', () => {
	const tagline = (form: HTMLFormElement) => ({ tagline: form.querySelector('input')?.value ?? '' });

	const taglineForm = (value: string) => {
		const form = document.createElement('form');
		form.innerHTML = '<input name="tagline">';
		form.querySelector('input')!.value = value;
		document.body.append(form);

		return form;
	};

	const edit = async (form: HTMLFormElement, value: string) => {
		const input = form.querySelector('input')!;
		input.value = value;
		input.dispatchEvent(new Event('input', { bubbles: true }));
		await tick();
	};

	afterEach(() => document.body.replaceChildren());

	it("treats a stored value it can't read as no draft", async () => {
		localStorage.setItem(KEY, '{not json');
		const draft = new FormDraft(() => KEY, tagline);

		draft.attach(taglineForm(''));
		await tick();

		expect([draft.values, draft.revision, hasDraft(KEY)]).toEqual([null, 0, false]);
	});

	it('drops the draft once the form went through, and keeps what was sent as its own values', async () => {
		const draft = new FormDraft(() => KEY, tagline);
		const form = taglineForm('Hypnotic');
		draft.attach(form);
		await tick();
		await edit(form, 'Rolling techno');
		expect(hasDraft(KEY)).toBe(true);

		draft.sent(form);
		expect(hasDraft(KEY)).toBe(false);
		await edit(form, 'Rolling techno');

		expect(hasDraft(KEY)).toBe(false);
	});

	it('neither saves nor restores once detached before the form drew', async () => {
		localStorage.setItem(
			KEY,
			JSON.stringify({ savedAt: '2026-11-01T00:00:00Z', base: '', values: { tagline: 'Old' } })
		);
		const draft = new FormDraft(() => KEY, tagline);
		const form = taglineForm('');

		draft.attach(form)?.();
		await tick();
		await edit(form, 'Rolling techno');

		expect([draft.values, stored()?.values]).toEqual([null, { tagline: 'Old' }]);
	});

	it('does nothing without a key', async () => {
		const redrawn: string[] = [];

		const draft = new FormDraft(
			() => undefined,
			tagline,
			() => redrawn.push('redraw')
		);

		const form = taglineForm('');

		expect(draft.attach(form)).toBeUndefined();
		draft.sent(form);
		draft.discard();

		expect([redrawn, draft.revision, Object.keys(localStorage).filter((key) => key.startsWith('ln:draft:'))]).toEqual([
			['redraw'],
			1,
			[]
		]);
	});

	it('saves what the form holds once edits made together have drawn', async () => {
		const draft = new FormDraft(() => KEY, tagline);
		const form = taglineForm('');
		draft.attach(form);
		await tick();
		const input = form.querySelector('input')!;

		input.value = 'Rolling';
		input.dispatchEvent(new Event('input', { bubbles: true }));
		input.value = 'Rolling techno';
		input.dispatchEvent(new Event('change', { bubbles: true }));
		await tick();

		expect(stored()?.values).toEqual({ tagline: 'Rolling techno' });
	});
});

describe('clearSignedOutDrafts', () => {
	const pat = draftKey({ did: 'did:plc:pat' }, 'project:new')!;

	afterEach(() => {
		localStorage.removeItem(pat);
		document.cookie = `${SIGNED_OUT_COOKIE}=; Max-Age=0; Path=/`;
	});

	it("drops the drafts of the accounts that signed out, and the cookie naming them, keeping everyone else's", () => {
		localStorage.setItem(KEY, '{}');
		localStorage.setItem(pat, '{}');
		document.cookie = `${SIGNED_OUT_COOKIE}=${encodeURIComponent('did:plc:neon did:plc:ada')}; Path=/`;

		clearSignedOutDrafts();

		expect([hasDraft(KEY), hasDraft(pat), document.cookie.includes(SIGNED_OUT_COOKIE)]).toEqual([false, true, false]);
	});

	it('keeps every draft when nobody signed out', () => {
		localStorage.setItem(KEY, '{}');

		clearSignedOutDrafts();

		expect(hasDraft(KEY)).toBe(true);
	});
});

describe('DraftNotice', () => {
	it('says nothing of files for a form without any', async () => {
		const draft = new FormDraft<{ tagline?: string }>(
			() => KEY,
			() => ({})
		);

		draft.values = { tagline: 'Rolling techno' };
		draft.savedAt = new Date().toISOString();

		await render(DraftNotice, { draft });

		await expect.element(notice()).toBeVisible();
		await expect.element(notice().getByText(/Pick any files again/)).not.toBeInTheDocument();
	});
});
