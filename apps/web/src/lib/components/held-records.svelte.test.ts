import { page } from 'vitest/browser';
import { describe, expect, it } from 'vitest';
import { render } from 'vitest-browser-svelte';
import type { HeldRecordView } from '#lib/types.ts';
import HeldRecordList from './HeldRecordList.svelte';

const record = (uri: string, overrides: Partial<HeldRecordView> = {}): HeldRecordView => ({
	uri,
	kind: 'booking',
	title: 'Message in Rooftop',
	excerpt: 'Any news?',
	createdAt: '2026-10-30T20:00:00Z',
	...overrides
});

const records = [
	record('at://m1'),
	record('at://m2', { title: 'Inquiry in Rooftop', excerpt: 'Free on the 5th?' }),
	record('at://r1', { kind: 'request', title: 'Claim: Neon Tide', excerpt: null })
];

/** The form a button submits, as the action and fields it posts. */
const posts = (name: string) => {
	const form = page.getByRole('button', { name }).element().closest('form')!;

	return [form.getAttribute('action'), Object.fromEntries(new FormData(form))];
};

/** A promise that never settles, as a list that's still loading. */
const pending = () => new Promise<HeldRecordView[]>(() => {});

describe('HeldRecordList', () => {
	it('groups the records by kind, each with a delete, and a delete for the whole kind', async () => {
		await render(HeldRecordList, { records: Promise.resolve(records) });

		const headings = page.getByRole('heading', { level: 2 });
		await expect.element(headings.first()).toHaveTextContent('Review requests1');
		expect(headings.elements().map((heading) => heading.textContent)).toEqual([
			'Review requests1',
			'Booking messages2'
		]);
		await expect
			.element(page.getByText('Deleting a pending one withdraws it. A decided one keeps its decision.'))
			.toBeVisible();
		await expect.element(page.getByText('Free on the 5th?')).toBeVisible();

		expect(posts('Delete Inquiry in Rooftop')).toEqual(['?/delete', { uri: 'at://m2' }]);
		expect(posts('Delete all booking messages')).toEqual(['?/deleteAll', { kind: 'booking' }]);
		expect(posts('Delete all review requests')).toEqual(['?/deleteAll', { kind: 'request' }]);
	});

	it('holds a placeholder while loading, and keeps the list while a new one loads', async () => {
		const { rerender } = await render(HeldRecordList, { records: pending() });

		await expect.element(page.getByText('Loading your data')).toBeInTheDocument();
		expect(page.getByRole('button').elements()).toHaveLength(0);

		await rerender({ records: Promise.resolve(records) });
		await expect.element(page.getByRole('button', { name: 'Delete Claim: Neon Tide' })).toBeVisible();

		await rerender({ records: pending(), form: { success: 'Deleted.' } });
		await expect.element(page.getByRole('status')).toHaveTextContent('Deleted.');
		await expect.element(page.getByRole('button', { name: 'Delete Claim: Neon Tide' })).toBeVisible();
		await expect.element(page.getByText('Loading your data')).not.toBeInTheDocument();
	});

	it('says when there is nothing to delete', async () => {
		await render(HeldRecordList, { records: Promise.resolve([]) });

		await expect.element(page.getByText('Nothing here')).toBeVisible();
		await expect.element(page.getByText("Local Network isn't holding anything for you.")).toBeVisible();
		expect(page.getByRole('button').elements()).toHaveLength(0);
	});

	it('offers to try again when the list fails to load', async () => {
		const failed = Promise.reject(new Error('directory down'));
		// Handled here as SvelteKit's streaming does, so the browser doesn't report it before the component reads it.
		failed.catch(() => {});
		await render(HeldRecordList, { records: failed });

		await expect.element(page.getByText("Couldn't load your data")).toBeVisible();
		await expect.element(page.getByRole('link', { name: 'Try again' })).toHaveAttribute('href', '/dashboard/data');
	});

	it('shows why a delete failed', async () => {
		await render(HeldRecordList, { records: Promise.resolve(records), form: { error: "That isn't one of yours" } });

		await expect.element(page.getByRole('alert')).toHaveTextContent("That isn't one of yours");
	});
});
