<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import { expect, userEvent, within } from 'storybook/test';
	import type { LineupEntry } from '#lib/events.ts';
	import { phone } from '#lib/testing/viewports.ts';
	import LineupFields from './LineupFields.svelte';

	const { Story } = defineMeta({ title: 'Patterns/Lineup fields', component: LineupFields, tags: ['autodocs'] });

	const saved: LineupEntry[] = [
		{ name: 'Neon Tide', href: '/@neon.test', value: '@neon.test', start: '23:00', end: '01:00' },
		{ name: 'Sam', href: '/@sam.test', value: '@sam.test', start: '01:00' },
		{ name: 'Night Shift Residents', href: '/act/3lzact/residents', value: 'https://localnetwork.test/act/3lzact/residents' }
	];

	/** The 50 an event holds. */
	const full: LineupEntry[] = Array.from({ length: 50 }, (_, index) => ({ name: `DJ ${index + 1}`, href: null, value: `@dj${index + 1}.test` }));

	/** What each row's field under `label` holds. */
	const values = (canvasElement: HTMLElement, label: string) => within(canvasElement).getAllByLabelText<HTMLInputElement>(label).map((input) => input.value);

	/** With nothing saved, one empty row. */
	const oneBlank = async ({ canvasElement }: { canvasElement: HTMLElement }) => {
		const canvas = within(canvasElement);

		await expect(values(canvasElement, 'Performer')).toEqual(['']);
		await expect(canvas.getByLabelText('Set starts')).toHaveValue('');
	};

	/** Adding a row gives an empty one; removing one takes out that row and keeps the rest with their times. */
	const addAndRemove = async ({ canvasElement }: { canvasElement: HTMLElement }) => {
		const canvas = within(canvasElement);

		await userEvent.click(canvas.getByRole('button', { name: 'Add to lineup' }));
		await expect(values(canvasElement, 'Performer')).toEqual([...saved.map((entry) => entry.value), '']);

		await userEvent.click(canvas.getAllByRole('button', { name: 'Remove from lineup' })[0]);
		await expect(values(canvasElement, 'Performer')).toEqual([...saved.slice(1).map((entry) => entry.value), '']);
		await expect(values(canvasElement, 'Set starts')).toEqual(['01:00', '', '']);
	};

	/** At 50 there's no adding another. */
	const atTheLimit = async ({ canvasElement }: { canvasElement: HTMLElement }) => {
		const canvas = within(canvasElement);

		await expect(canvas.getAllByLabelText('Performer')).toHaveLength(50);
		await expect(canvas.queryByRole('button', { name: 'Add to lineup' })).toBeNull();
	};

	/** A refused save says why, and the performer fields point to it. */
	const refused = async ({ canvasElement }: { canvasElement: HTMLElement }) => {
		const canvas = within(canvasElement);

		await expect(canvas.getAllByLabelText('Performer')[0]).toHaveAccessibleDescription("There's no account called @nobody.test");
		await expect(canvas.getAllByLabelText('Performer')[0]).toHaveAttribute('aria-invalid', 'true');
	};

	/** While its form sends, the fields can't be changed. */
	const locked = async ({ canvasElement }: { canvasElement: HTMLElement }) => {
		await expect(within(canvasElement).getAllByLabelText('Performer')[0]).toHaveAttribute('readonly');
	};
</script>

<Story name="New event" play={oneBlank} />

<Story name="Saved lineup" args={{ saved }} />

<Story name="Adding and removing" args={{ saved }} play={addAndRemove} />

<Story name="At the limit" args={{ saved: full }} play={atTheLimit} />

<Story name="Refused" args={{ saved, error: "There's no account called @nobody.test" }} play={refused} />

<Story name="Sending" args={{ saved, readonly: true }} play={locked} />

<Story name="Phone" args={{ saved }} globals={phone} />
