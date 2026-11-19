import { page, userEvent } from 'vitest/browser';
import { describe, expect, it } from 'vitest';
import { render } from 'vitest-browser-svelte';
import PictureField from './PictureField.svelte';

const png = (name: string) => new File(['png'], name, { type: 'image/png' });

const shown = () => document.querySelector('label img')?.getAttribute('src');

describe('PictureField', () => {
	it('shows why the server refused the picture', async () => {
		await render(PictureField, { error: 'Use a PNG, JPEG or WebP picture' });

		await expect.element(page.getByText('Use a PNG, JPEG or WebP picture')).toBeInTheDocument();
		expect(page.getByRole('checkbox', { name: 'Remove' }).elements()).toHaveLength(0);
	});

	it('shows the latest picked picture, and the saved one again once the pick is cleared', async () => {
		await render(PictureField, { src: '/media/did:plc:root/bafkreiavatar' });
		const input = page.getByLabelText('Picture');

		await userEvent.upload(input, png('first.png'));
		const first = shown();
		await userEvent.upload(input, png('second.png'));

		expect(shown()).toMatch(/^blob:/);
		expect(shown()).not.toBe(first);

		await userEvent.upload(input, []);
		expect(shown()).toBe('/media/did:plc:root/bafkreiavatar');
		await expect.element(page.getByRole('checkbox', { name: 'Remove' })).toBeInTheDocument();
	});
});
