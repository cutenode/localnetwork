<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import { expect, userEvent, within } from 'storybook/test';
	import { phone } from '#lib/testing/viewports.ts';
	import DeleteRow from './DeleteRow.svelte';
	import FormActions from './FormActions.svelte';
	import FormActionsHarness from './FormActionsHarness.svelte';

	const { Story } = defineMeta({ title: 'Patterns/Form actions', component: FormActionsHarness, tags: ['autodocs'] });

	const refused = { error: "That doesn't look like a web address", field: { name: 'name', row: 0 } };

	const longError = { error: 'Give each extra mix a name and a web address, or clear the row you started' };

	/** Edits the name, and the bar says the change hasn't gone anywhere yet: `note` is what it says. */
	const edit =
		(note: string) =>
		async ({ canvasElement }: { canvasElement: HTMLElement }) => {
			const canvas = within(canvasElement);

			await userEvent.type(canvas.getByLabelText('Name'), ' (live)');
			await expect(await canvas.findByText(note)).toBeVisible();
		};
</script>

<Story name="Nothing changed" args={{ values: { name: 'Neon Tide' } }} />

<Story name="Unsaved changes" args={{ values: { name: 'Neon Tide' } }} play={edit('Unsaved changes')} />

<Story name="Kept as a draft" args={{ values: { name: 'Neon Tide' }, kept: true }} play={edit('Kept in this browser')} />

<Story name="Sending" args={{ values: { name: 'Neon Tide' }, submitting: true }} />

<Story name="Refused" args={{ values: { name: 'Neon Tide' }, form: longError }} />

<Story name="Refused, phone" args={{ values: { name: 'Neon Tide' }, form: longError }} globals={phone} />

<Story name="Refused control" args={{ values: { name: 'Neon Tide' }, form: refused }} />

<Story name="Saved" args={{ values: { name: 'Neon Tide' }, form: { success: 'Saved.' } }} />

<Story name="Saved, phone" args={{ values: { name: 'Neon Tide' }, form: { success: 'Saved.' } }} globals={phone} />

<!-- On a phone, each button takes an equal share of the row. -->
<Story name="Three buttons, phone" globals={phone} asChild>
	<FormActions form={null}>
		<button type="button" class="btn-ghost">Preview</button>
		<button type="button" class="btn-ghost">Save draft</button>
		<button type="button" class="btn-primary">Publish</button>
	</FormActions>
</Story>

<Story name="Delete row" asChild>
	<DeleteRow action="?/delete" label="Delete show" note="Deleting takes it off every listing. To tell people it's off, mark it cancelled instead." warning="This takes it off every listing, and it can't be undone." />
</Story>
