import { page, userEvent } from 'vitest/browser';
import { describe, expect, it } from 'vitest';
import { render } from 'vitest-browser-svelte';
import GenreInput from './GenreInput.svelte';

const typed = () => page.getByRole('combobox');

const posted = (name = 'genres') => document.querySelector<HTMLInputElement>(`input[type=hidden][name=${name}]`)?.value;

describe('GenreInput', () => {
	it('adds a genre on a comma, once, and no more than the most it takes', async () => {
		await render(GenreInput, { value: 'techno', max: 2 });

		await typed().fill('Techno');
		await userEvent.keyboard(',');
		await typed().fill('dub, ukg');
		await userEvent.keyboard(',');

		expect(posted()).toBe('techno, dub');
		await expect.element(typed()).toHaveValue('');
	});

	it('takes back the last genre on Backspace in an empty field, and only then', async () => {
		await render(GenreInput, { value: 'house, techno' });

		await typed().fill('d');
		await userEvent.keyboard('{Backspace}');
		expect(posted()).toBe('house, techno');
		await userEvent.keyboard('{Backspace}');

		expect(posted()).toBe('house');
		await expect.element(page.getByRole('button', { name: 'Remove techno' })).not.toBeInTheDocument();
	});

	it('leaves the genres alone on Backspace once there are none', async () => {
		await render(GenreInput);

		await typed().click();
		await userEvent.keyboard('{Backspace}');

		expect(posted()).toBe('');
		await expect.element(typed()).toHaveAttribute('placeholder', 'Type a genre and press Enter');
	});

	it('adds what was typed when the field loses focus', async () => {
		await render(GenreInput, { name: 'sounds', id: 'sounds' });

		await typed().fill('Hip Hop');
		await userEvent.tab();
		await typed().click();
		await userEvent.tab();

		expect(posted('sounds')).toBe('hip-hop');
		await expect.element(typed()).toHaveAttribute('placeholder', '');
	});

	it('ignores other keys', async () => {
		await render(GenreInput);

		await typed().fill('disco');
		await userEvent.keyboard('{ArrowLeft}');

		expect(posted()).toBe('');
		await expect.element(typed()).toHaveValue('disco');
	});
});
