<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import { expect, userEvent, within } from 'storybook/test';
	import { light } from '#lib/testing/light.ts';
	import { phone } from '#lib/testing/viewports.ts';
	import PictureField from './PictureField.svelte';

	const { Story } = defineMeta({ title: 'Patterns/Picture field', component: PictureField, tags: ['autodocs'] });

	/** A picture already on the page, drawn inline so the story never loads one. */
	const src = `data:image/svg+xml,${encodeURIComponent('<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1 1"><rect width="1" height="1" fill="#2a6f6f"/></svg>')}`;

	const error = 'Images can be at most 1 MB';

	/** With nothing saved there's nothing to take off. */
	const nothingToRemove = async ({ canvasElement }: { canvasElement: HTMLElement }) => {
		await expect(within(canvasElement).queryByRole('checkbox', { name: 'Remove' })).toBeNull();
	};

	/** A saved picture can be taken off. */
	const removable = async ({ canvasElement }: { canvasElement: HTMLElement }) => {
		await expect(within(canvasElement).getByRole('checkbox', { name: 'Remove' })).not.toBeChecked();
	};

	/** Picking a file shows it at once, in place of the saved one, and there's no saved one left to remove. */
	const pick = async ({ canvasElement }: { canvasElement: HTMLElement }) => {
		const canvas = within(canvasElement);

		await userEvent.upload(canvas.getByLabelText('Picture'), new File(['png'], 'flyer.png', { type: 'image/png' }));
		await expect(canvas.getByRole('presentation')).toHaveAttribute('src', expect.stringMatching(/^blob:/));
		await expect(canvas.queryByRole('checkbox', { name: 'Remove' })).toBeNull();
	};

	/** The server's reason shows under the box, and the field is marked invalid. */
	const refused = async ({ canvasElement }: { canvasElement: HTMLElement }) => {
		const canvas = within(canvasElement);

		await expect(canvas.getByText(error)).toBeVisible();
		await expect(canvas.getByLabelText('Picture')).toHaveAttribute('aria-invalid', 'true');
	};

	/** While the page saves, neither the picture nor its removal can change. */
	const saving = async ({ canvasElement }: { canvasElement: HTMLElement }) => {
		const canvas = within(canvasElement);

		await expect(canvas.getByLabelText('Picture')).toBeDisabled();
		await expect(canvas.getByRole('checkbox', { name: 'Remove' })).toBeDisabled();
	};
</script>

<Story name="Empty" play={nothingToRemove} />

<Story name="With a picture" args={{ src }} play={removable} />

<Story name="Picking a new picture" args={{ src }} play={pick} />

<Story name="Refused" args={{ error }} play={refused} />

<Story name="Saving" args={{ src, readonly: true }} play={saving} />

<Story name="Phone, refused, light" args={{ src, error }} globals={phone} beforeEach={light} play={refused} />
