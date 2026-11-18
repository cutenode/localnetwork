<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import { expect, userEvent, within } from 'storybook/test';
	import { phone } from '#lib/testing/viewports.ts';
	import GenreInput from './GenreInput.svelte';

	const { Story } = defineMeta({ title: 'Patterns/Genre input', component: GenreInput, tags: ['autodocs'] });

	const many = 'house, deep house, techno, dub techno, electro, breaks, jungle, footwork, ambient, disco';

	/** Types two genres the way someone would, each ended with Enter or a comma, and they become tags. */
	const typeTwo = async ({ canvasElement }: { canvasElement: HTMLElement }) => {
		const canvas = within(canvasElement);

		await userEvent.type(canvas.getByLabelText('Genres'), 'Techno{Enter}dub techno,');
		await expect(canvas.getByRole('button', { name: 'Remove techno' })).toBeVisible();
		await expect(canvas.getByRole('button', { name: 'Remove dub techno' })).toBeVisible();
	};

	/** A genre past `max` doesn't become a tag. */
	const oneTooMany = async ({ canvasElement }: { canvasElement: HTMLElement }) => {
		const canvas = within(canvasElement);

		await userEvent.type(canvas.getByLabelText('Genres'), 'jungle{Enter}');
		await expect(canvas.queryByRole('button', { name: 'Remove jungle' })).toBeNull();
	};
</script>

{#snippet field(value: string, max?: number)}
	<div class="max-w-xl">
		<label class="label" for="genres">Genres</label>
		<GenreInput {value} {max} />
	</div>
{/snippet}

<Story name="Empty" asChild>{@render field('')}</Story>

<Story name="Typing genres" play={typeTwo} asChild>{@render field('')}</Story>

<Story name="With genres" asChild>{@render field('techno, dub techno, electro')}</Story>

<Story name="At the limit" play={oneTooMany} asChild>{@render field('techno, dub techno, electro', 3)}</Story>

<Story name="Phone" globals={phone} asChild>{@render field(many)}</Story>
