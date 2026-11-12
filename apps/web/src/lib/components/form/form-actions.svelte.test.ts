import { flushSync } from 'svelte';
import { page, userEvent } from 'vitest/browser';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { render } from 'vitest-browser-svelte';
import { stubFormAction } from '#lib/testing/form-action.ts';
import DeleteRow from './DeleteRow.svelte';
import FormActionsHarness from './FormActionsHarness.svelte';

type Navigation = { willUnload: boolean; cancel: () => void };

/** Stands in for SvelteKit's navigations, which don't run outside an app: keeps the guard the bar registers. */
const leaving = { guard: (_navigation: Navigation) => {} };

const guardNavigation = (guard: (navigation: Navigation) => void) => {
	leaving.guard = guard;
};

/** What the guard does with a navigation: whether it held it back. */
function held(willUnload = false) {
	const cancel = vi.fn<() => void>();
	leaving.guard({ willUnload, cancel });

	return cancel.mock.calls.length > 0;
}

const name = page.getByRole('textbox', { name: 'Name' });

describe('FormActions', () => {
	beforeEach(() => vi.unstubAllGlobals());

	it('names changes not sent yet, and drops the line when they are undone', async () => {
		await render(FormActionsHarness, { guardNavigation, values: { name: 'Neon Tide' } });
		await expect.element(page.getByText('Unsaved changes')).not.toBeInTheDocument();

		await name.fill('Neon Tides');
		await expect.element(page.getByText('Unsaved changes')).toBeVisible();

		await name.fill('Neon Tide');
		await expect.element(page.getByText('Unsaved changes')).not.toBeInTheDocument();
	});

	it('counts a row added without any typing as a change', async () => {
		await render(FormActionsHarness, { guardNavigation });

		await userEvent.click(page.getByRole('button', { name: 'Add row' }));

		await expect.element(page.getByText('Unsaved changes')).toBeVisible();
	});

	it('says a draft keeps the changes, and lets people leave', async () => {
		vi.stubGlobal('confirm', () => {
			throw new Error('a kept form should not ask');
		});
		await render(FormActionsHarness, { guardNavigation, kept: true });

		await name.fill('Neon Tides');

		await expect.element(page.getByText('Kept in this browser')).toBeVisible();
		expect(held()).toBe(false);
	});

	it('asks before leaving with changes, and stays if the answer is no', async () => {
		const confirm = vi.fn<() => boolean>(() => false);
		vi.stubGlobal('confirm', confirm);
		await render(FormActionsHarness, { guardNavigation });
		expect(held()).toBe(false);
		expect(confirm).not.toHaveBeenCalled();

		await name.fill('Neon Tides');
		await expect.element(page.getByText('Unsaved changes')).toBeVisible();

		expect(held()).toBe(true);
		confirm.mockReturnValue(true);
		expect(held()).toBe(false);
	});

	it('leaves the question to the browser when the page itself unloads', async () => {
		const confirm = vi.fn<() => boolean>();
		vi.stubGlobal('confirm', confirm);
		await render(FormActionsHarness, { guardNavigation });

		await name.fill('Neon Tides');
		await expect.element(page.getByText('Unsaved changes')).toBeVisible();

		expect(held(true)).toBe(true);
		expect(confirm).not.toHaveBeenCalled();
	});

	it('lets a send through without asking', async () => {
		const confirm = vi.fn<() => boolean>();
		vi.stubGlobal('confirm', confirm);
		const { rerender } = await render(FormActionsHarness, { guardNavigation });

		await name.fill('Neon Tides');
		await expect.element(page.getByText('Unsaved changes')).toBeVisible();
		await rerender({ submitting: true });

		expect(held()).toBe(false);
		expect(confirm).not.toHaveBeenCalled();
	});

	it('takes what was saved as the new start', async () => {
		const { rerender } = await render(FormActionsHarness, { guardNavigation });

		await name.fill('Neon Tides');
		await expect.element(page.getByText('Unsaved changes')).toBeVisible();

		await rerender({ submitting: true });
		await rerender({ submitting: false, form: { success: 'Saved.' } });

		await expect.element(page.getByText('Saved.')).toBeVisible();
		await expect.element(page.getByText('Unsaved changes')).not.toBeInTheDocument();
		expect(held()).toBe(false);
	});

	it('keeps the changes when the send is refused, and hands focus to the reason', async () => {
		const { rerender } = await render(FormActionsHarness, { guardNavigation });

		await name.fill('Neon Tides');
		await userEvent.click(page.getByRole('button', { name: 'Send' }));
		await rerender({ submitting: true });

		// The disabled button drops focus, as it does on a page.
		if (document.activeElement instanceof HTMLElement) document.activeElement.blur();
		await rerender({ submitting: false, form: { error: 'That name is taken' } });

		await expect.element(page.getByRole('alert')).toHaveTextContent('That name is taken');
		await expect.poll(() => document.activeElement?.contains(page.getByRole('alert').element())).toBe(true);

		await name.fill('Neon Tide');
		await name.fill('Neon Tidal');
		expect(page.getByRole('alert').element()).toBeTruthy();
	});

	it('marks the refused control, says why beside it, and clears both on the next edit', async () => {
		const { rerender } = await render(FormActionsHarness, { guardNavigation });

		await rerender({ submitting: true });
		await rerender({ submitting: false, form: { error: 'Add a real name', field: { name: 'name', row: 0 } } });

		await expect.element(name).toHaveAttribute('aria-invalid', 'true');
		await expect.element(name).toHaveFocus();
		await expect.element(name).toHaveAccessibleDescription('Add a real name');
		await expect.element(page.getByText('Add a real name').first()).toBeVisible();

		await name.fill('Neon Tide');

		await expect.element(name).not.toHaveAttribute('aria-invalid');
		await expect.element(name).not.toHaveAttribute('aria-describedby');
		expect(page.getByText('Add a real name').elements()).toHaveLength(1);
	});

	it('puts a description back once the control is fixed', async () => {
		const { rerender } = await render(FormActionsHarness, { guardNavigation });
		const hinted = page.getByRole('textbox', { name: 'Hinted' });

		await rerender({ submitting: true });
		await rerender({ submitting: false, form: { error: 'Too long', field: { name: 'hinted', row: 0 } } });
		await expect.element(hinted).toHaveAccessibleDescription('Too long');

		await hinted.fill('Short');

		await expect.element(hinted).toHaveAccessibleDescription('A hint');
	});

	it('finds the row a repeated control was refused in', async () => {
		const { rerender } = await render(FormActionsHarness, { guardNavigation, rows: 3 });

		await rerender({ submitting: true });
		await rerender({ submitting: false, form: { error: 'Not a link', field: { name: 'row', row: 1 } } });

		await expect.element(page.getByRole('textbox', { name: 'Row 2' })).toHaveAttribute('aria-invalid', 'true');
		await expect.element(page.getByRole('textbox', { name: 'Row 1' })).not.toHaveAttribute('aria-invalid');
	});

	it('marks the visible input in place of a hidden one, and the reason goes under its box', async () => {
		const { rerender } = await render(FormActionsHarness, { guardNavigation });

		await rerender({ submitting: true });
		await rerender({ submitting: false, form: { error: 'Add a genre', field: { name: 'genres', row: 0 } } });

		const shown = page.getByRole('textbox', { name: 'Genres' });
		await expect.element(shown).toHaveFocus();
		expect(shown.element().parentElement?.parentElement?.lastElementChild?.textContent).toBe('Add a genre');
	});

	it('still says why when the control it names is gone', async () => {
		const { rerender } = await render(FormActionsHarness, { guardNavigation });

		await rerender({ submitting: true });
		await rerender({ submitting: false, form: { error: 'No such field', field: { name: 'gone', row: 0 } } });

		await expect.element(page.getByRole('alert')).toHaveTextContent('No such field');
	});

	it('leaves a lapsed sign-in to SignInAgain', async () => {
		await render(FormActionsHarness, { guardNavigation, form: { error: 'Sign in again to save', reauth: true } });

		await expect.element(page.getByRole('alert')).not.toBeInTheDocument();
	});

	it('draws nothing to track outside a form, and comes off cleanly', async () => {
		const { unmount } = await render(FormActionsHarness, { guardNavigation, bare: true });
		await expect.element(page.getByRole('button', { name: 'Send' })).toBeVisible();

		await unmount();
		flushSync();

		await expect.element(page.getByRole('button', { name: 'Send' })).not.toBeInTheDocument();
	});
});

describe('DeleteRow', () => {
	const props = {
		action: '?/delete',
		label: 'Delete show',
		note: 'Deleting takes it off.',
		warning: "This can't be undone."
	};

	it('warns first, and only then offers the button that posts', async () => {
		const action = stubFormAction();
		await render(DeleteRow, props);

		await expect.element(page.getByText('Deleting takes it off.')).toBeVisible();
		await expect.element(page.getByRole('button', { name: 'Delete show' })).not.toBeInTheDocument();

		await userEvent.click(page.getByRole('button', { name: 'Delete…' }));
		await expect.element(page.getByText("This can't be undone.")).toBeVisible();
		expect(action.sent).toHaveLength(0);
		expect(page.getByRole('button', { name: 'Delete show' }).element().closest('form')?.getAttribute('action')).toBe(
			'?/delete'
		);
	});

	it('goes back to the note when kept', async () => {
		await render(DeleteRow, props);

		await userEvent.click(page.getByRole('button', { name: 'Delete…' }));
		await userEvent.click(page.getByRole('button', { name: 'Keep it' }));

		await expect.element(page.getByText('Deleting takes it off.')).toBeVisible();
		await expect.element(page.getByRole('button', { name: 'Delete…' })).toBeVisible();
	});
});
