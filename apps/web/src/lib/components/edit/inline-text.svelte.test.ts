import { page, userEvent } from 'vitest/browser';
import { describe, expect, it } from 'vitest';
import { render } from 'vitest-browser-svelte';
import InlineText from './InlineText.svelte';

describe('InlineText', () => {
	it('keeps a single-line value on one line, and adds lines only when multiline', async () => {
		await render(InlineText, { label: 'Name', name: 'name', value: 'Night' });
		const single = page.getByLabelText('Name');
		await userEvent.type(single, '{End}{Enter}Shift');
		await expect.element(single).toHaveValue('NightShift');

		await render(InlineText, { label: 'About', name: 'about', value: 'One', multiline: true });
		const multi = page.getByLabelText('About');
		await userEvent.type(multi, '{End}{Enter}Two');
		await expect.element(multi).toHaveValue('One\nTwo');
	});

	it('takes its type from class and says why a save was refused', async () => {
		await render(InlineText, {
			label: 'Title',
			name: 'title',
			class: 'text-heading',
			error: 'Too long',
			required: true
		});
		const field = page.getByLabelText('Title (required)');

		await expect.element(field).toHaveClass('text-heading');
		await expect.element(field).toHaveAttribute('aria-invalid', 'true');
		await expect.element(field).toHaveAccessibleDescription('Too long');
	});

	it('draws just the box when a page hands it no class', async () => {
		// A caller passing an unset field's class through gets null, not the default.
		// @ts-expect-error -- untyped markup can hand it null, which the props don't admit
		await render(InlineText, { label: 'Venue', name: 'venue', class: null });

		const field = page.getByLabelText('Venue');
		await expect.element(field).toHaveClass('field-sizing-content');
		await expect.element(field).not.toHaveClass('null');
	});
});
