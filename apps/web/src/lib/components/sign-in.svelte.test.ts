import { page, userEvent } from 'vitest/browser';
import { describe, expect, it, vi } from 'vitest';
import { render } from 'vitest-browser-svelte';
import type { ActorSuggestion } from '#lib/actors.ts';
import { CAN, CANNOT } from '#lib/sign-in.ts';
import HandleInput from './HandleInput.svelte';
import HowSignInWorks from './HowSignInWorks.svelte';

const accounts: ActorSuggestion[] = [
	{ did: 'did:plc:neon', handle: 'neon.test', displayName: 'Neon Tide', avatar: '/favicon.svg' },
	{ did: 'did:plc:nell', handle: 'nell.test' }
];

describe('HandleInput', () => {
	const field = () => page.getByRole('combobox', { name: 'Your handle' });

	it('suggests accounts as you type and picks one with the arrow keys and Enter', async () => {
		const search = vi.fn<(query: string, signal: AbortSignal) => Promise<ActorSuggestion[]>>(async () => accounts);
		await render(HandleInput, { search });

		await userEvent.type(field(), '@ne');
		await expect.element(page.getByRole('listbox', { name: 'Accounts' })).toBeVisible();
		expect(search).toHaveBeenCalledTimes(1);
		expect(search).toHaveBeenCalledWith('ne', expect.any(AbortSignal));
		await expect
			.element(page.getByRole('option', { name: 'Neon Tide @neon.test' }))
			.toHaveAttribute('aria-selected', 'false');
		expect(
			page.getByRole('option', { name: 'Neon Tide @neon.test' }).element().querySelector('img')?.getAttribute('src')
		).toBe('/favicon.svg');
		await expect
			.element(page.getByRole('status'))
			.toHaveTextContent('2 accounts found. Use the arrow keys to pick one.');
		await expect.element(field()).toHaveAttribute('aria-expanded', 'true');

		await userEvent.keyboard('{ArrowDown}{ArrowDown}');
		await expect
			.element(page.getByRole('option', { name: 'nell.test @nell.test' }))
			.toHaveAttribute('aria-selected', 'true');
		const activeId = page.getByRole('option', { name: 'nell.test @nell.test' }).element().id;
		await expect.element(field()).toHaveAttribute('aria-activedescendant', activeId);

		await userEvent.keyboard('{ArrowDown}{ArrowUp}{ArrowUp}{ArrowDown}');
		await expect
			.element(page.getByRole('option', { name: 'nell.test @nell.test' }))
			.toHaveAttribute('aria-selected', 'true');

		await userEvent.keyboard('{Enter}');
		await expect.element(field()).toHaveValue('nell.test');
		await expect.element(field()).toHaveAttribute('aria-expanded', 'false');
		await expect.element(page.getByRole('listbox', { includeHidden: true })).not.toBeVisible();
	});

	it('closes on Escape, reopens on ArrowDown, and fills the handle from a click', async () => {
		await render(HandleInput, { search: async () => accounts });

		await userEvent.type(field(), 'neo');
		await expect.element(page.getByRole('listbox')).toBeVisible();
		await userEvent.keyboard('{Escape}');
		await expect.element(field()).toHaveAttribute('aria-expanded', 'false');
		await expect.element(field()).toHaveValue('neo');

		await userEvent.keyboard('{ArrowDown}');
		await expect
			.element(page.getByRole('option', { name: 'Neon Tide @neon.test' }))
			.toHaveAttribute('aria-selected', 'true');
		await page.getByRole('option', { name: 'Neon Tide @neon.test' }).click();
		await expect.element(field()).toHaveValue('neon.test');
		await expect.element(field()).toHaveFocus();
	});

	it('waits for two characters, leaves Enter to the form, and keeps the field usable when the search fails', async () => {
		const search = vi.fn<(query: string, signal: AbortSignal) => Promise<ActorSuggestion[]>>(async (query) => {
			if (query === 'xx') throw new Error('offline');

			return [];
		});

		await render(HandleInput, { search, value: 'n', invalid: true, describedby: 'handle-error' });

		await expect.element(field()).toHaveAttribute('aria-invalid', 'true');
		await expect.element(field()).toHaveAttribute('aria-describedby', 'handle-error');
		await userEvent.clear(field());
		await userEvent.type(field(), 'x');
		await new Promise((resolve) => setTimeout(resolve, 300));
		expect(search).not.toHaveBeenCalled();

		await userEvent.type(field(), 'x');
		await vi.waitFor(() => expect(search).toHaveBeenCalledWith('xx', expect.any(AbortSignal)));
		await userEvent.keyboard('{ArrowDown}');
		await expect.element(field()).toHaveAttribute('aria-expanded', 'false');
		await expect.element(field()).not.toHaveAttribute('aria-activedescendant');
	});
});

describe('HowSignInWorks', () => {
	it('opens from a command button and lists what Local Network can and cannot do', async () => {
		const { container } = await render(HowSignInWorks, { id: 'how' });
		const opener = document.createElement('button');
		opener.setAttribute('commandfor', 'how');
		opener.setAttribute('command', 'show-modal');
		opener.textContent = 'How signing in works';
		container.append(opener);

		await page.getByRole('button', { name: 'How signing in works' }).click();
		const dialog = page.getByRole('dialog', { name: 'How signing in works' });
		await expect.element(dialog).toBeVisible();

		for (const line of [...CAN.map((item) => item.text), ...CANNOT])
			await expect.element(dialog.getByText(line)).toBeVisible();

		await dialog.getByRole('button', { name: 'Close' }).click();
		await expect.element(dialog).not.toBeInTheDocument();
	});
});
