<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import { expect, userEvent, within } from 'storybook/test';
	import type { ProjectLink } from '#lib/types.ts';
	import LinkFields from './LinkFields.svelte';

	const { Story } = defineMeta({ title: 'Patterns/Link fields', component: LinkFields, tags: ['autodocs'] });

	const saved: ProjectLink[] = [
		{ kind: 'soundcloud', url: 'https://soundcloud.com/neon-tide' },
		{ kind: 'instagram', url: 'https://instagram.com/neontide', label: 'Neon Tide on Instagram' },
		{ kind: 'website', url: 'https://neontide.test' }
	];

	/** The 12 links the basics hold. */
	const full: ProjectLink[] = Array.from({ length: 12 }, (_, index) => ({ kind: 'website', url: `https://neontide.test/${index + 1}` }));

	const phone = { viewport: { value: 'mobile2', isRotated: false } };

	/** With nothing saved, one empty row of the `blank` type. */
	const oneBlank =
		(type: string) =>
		async ({ canvasElement }: { canvasElement: HTMLElement }) => {
			const canvas = within(canvasElement);

			await expect(canvas.getAllByLabelText('Link URL')).toHaveLength(1);
			await expect(canvas.getByLabelText('Link URL')).toHaveValue('');
			await expect(canvas.getByLabelText('Link type')).toHaveDisplayValue(type);
		};

	/** Adding a row gives an empty website link; removing one takes out that row and keeps the rest. */
	const addAndRemove = async ({ canvasElement }: { canvasElement: HTMLElement }) => {
		const canvas = within(canvasElement);
		const urls = () => canvas.getAllByRole<HTMLInputElement>('textbox', { name: 'Link URL' }).map((input) => input.value);

		await userEvent.click(canvas.getByRole('button', { name: 'Add link' }));
		await expect(urls()).toEqual([...saved.map((link) => link.url), '']);
		await expect(canvas.getAllByLabelText('Link type').at(-1)).toHaveDisplayValue('Website');

		await userEvent.click(canvas.getAllByRole('button', { name: 'Remove link' })[0]);
		await expect(urls()).toEqual([...saved.slice(1).map((link) => link.url), '']);
		await expect(canvas.getAllByRole<HTMLSelectElement>('combobox', { name: 'Link type' }).map((select) => select.value)).toEqual(['instagram', 'website', 'website']);
	};

	/** At 12 links there's no adding another. */
	const atTheLimit = async ({ canvasElement }: { canvasElement: HTMLElement }) => {
		const canvas = within(canvasElement);

		await expect(canvas.getAllByLabelText('Link URL')).toHaveLength(12);
		await expect(canvas.queryByRole('button', { name: 'Add link' })).toBeNull();
	};
</script>

<Story name="New profile" play={oneBlank('SoundCloud')} />

<Story name="New party" args={{ blank: 'website' }} play={oneBlank('Website')} />

<Story name="Saved links" args={{ saved }} />

<Story name="Adding and removing" args={{ saved }} play={addAndRemove} />

<Story name="At the limit" args={{ saved: full }} play={atTheLimit} />

<Story name="Phone" args={{ saved }} globals={phone} />
