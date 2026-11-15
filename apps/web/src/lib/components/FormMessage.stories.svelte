<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import { phone } from '#lib/testing/viewports.ts';
	import FormMessage from './FormMessage.svelte';

	const { Story } = defineMeta({ title: 'Patterns/Form message', component: FormMessage, tags: ['autodocs'] });

	const longError = { error: 'Give each extra mix a name and a web address, or clear the row you started' };
</script>

<Story name="Error" args={{ form: { error: "That doesn't look like a web address" } }} />

<Story name="Saved" args={{ form: { success: 'Saved.' } }} />

<Story name="Long error, phone" args={{ form: longError }} globals={phone} />
