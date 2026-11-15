<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import { expect, userEvent, within } from 'storybook/test';
	import { FormDraft } from '#lib/form-draft.svelte.ts';
	import { phone } from '#lib/testing/viewports.ts';
	import DraftNotice from './DraftNotice.svelte';

	const { Story } = defineMeta({ title: 'Patterns/Draft notice', component: DraftNotice, tags: ['autodocs'] });

	/** A draft restored from five minutes ago. With no storage key, discarding it only clears what it shows. */
	const restored = (stale = false) => {
		const draft = new FormDraft<{ title: string }>(
			() => undefined,
			() => ({ title: '' })
		);

		draft.values = { title: 'Rooftop Rave' };
		draft.savedAt = new Date(Date.now() - 5 * 60_000).toISOString();
		draft.stale = stale;

		return draft;
	};

	/** Discarding the draft takes the notice away. */
	const discard = async ({ canvasElement }: { canvasElement: HTMLElement }) => {
		const canvas = within(canvasElement);

		await userEvent.click(canvas.getByRole('button', { name: 'Discard' }));
		await expect(canvas.queryByRole('status')).toBeNull();
	};
</script>

<Story name="Changes back" args={{ draft: restored() }} />

<Story name="Saved version changed since" args={{ draft: restored(true) }} />

<Story name="With files" args={{ draft: restored(), files: true }} />

<Story name="Discarded" args={{ draft: restored() }} play={discard} />

<Story name="Phone" args={{ draft: restored(true), files: true }} globals={phone} />
