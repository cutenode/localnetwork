import { page } from 'vitest/browser';
import { describe, expect, it } from 'vitest';
import { posted, renderInForm } from '#lib/testing/forms.ts';
import ClaimFields from './ClaimFields.svelte';

const claimant = { did: 'did:plc:neon', handle: 'neon.test' };

describe('ClaimFields', () => {
	it('posts the proof typed, and starts over from new values', async () => {
		const { form, rerender } = await renderInForm(ClaimFields, { values: {}, claimant });
		const evidence = page.getByRole('textbox', { name: 'How can a moderator verify this?' });

		await evidence.fill('It is in our SoundCloud bio.');
		expect(posted(form, 'evidence')).toEqual({ evidence: ['It is in our SoundCloud bio.'] });

		await rerender({ values: { evidence: 'Posted from the project account.' }, claimant });
		await expect.element(evidence).toHaveValue('Posted from the project account.');
	});
});
