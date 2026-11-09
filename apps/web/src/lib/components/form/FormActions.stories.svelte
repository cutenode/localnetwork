<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import DeleteRow from './DeleteRow.svelte';
	import FormActionsHarness from './FormActionsHarness.svelte';

	const { Story } = defineMeta({ title: 'Patterns/Form actions', component: FormActionsHarness, tags: ['autodocs'] });

	const phone = { viewport: { value: 'mobile2', isRotated: false } };

	const refused = { error: "That doesn't look like a web address", field: { name: 'name', row: 0 } };

	const longError = { error: 'Give each extra mix a name and a web address, or clear the row you started' };
</script>

<Story name="Nothing changed" args={{ values: { name: 'Neon Tide' } }} />

<Story name="Sending" args={{ values: { name: 'Neon Tide' }, submitting: true }} />

<Story name="Refused" args={{ values: { name: 'Neon Tide' }, form: longError }} />

<Story name="Refused, phone" args={{ values: { name: 'Neon Tide' }, form: longError }} globals={phone} />

<Story name="Refused control" args={{ values: { name: 'Neon Tide' }, form: refused }} />

<Story name="Saved" args={{ values: { name: 'Neon Tide' }, form: { success: 'Saved.' } }} />

<Story name="Saved, phone" args={{ values: { name: 'Neon Tide' }, form: { success: 'Saved.' } }} globals={phone} />

<Story name="Delete row" asChild>
	<DeleteRow action="?/delete" label="Delete show" note="Deleting takes it off every listing. To tell people it's off, mark it cancelled instead." warning="This takes it off every listing, and it can't be undone." />
</Story>
