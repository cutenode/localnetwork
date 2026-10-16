<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import Button from './Button.svelte';
	import SelectField from './SelectField.svelte';
	import TextField from './TextField.svelte';

	const { Story } = defineMeta({ title: 'Primitives/Fields', component: TextField, tags: ['autodocs'] });

	const slots = [
		{ value: 'warmup', label: 'Warm-up' },
		{ value: 'peak', label: 'Peak time' },
		{ value: 'closing', label: 'Closing' }
	];
</script>

<Story name="Text" asChild>
	<div class="max-w-90"><TextField id="sound" label="Sound or name" placeholder="disco, jungle, a name" /></div>
</Story>

<Story name="With a hint" asChild>
	<div class="max-w-90"><TextField id="near" label="Near" hint="A city or postcode. Results sort by distance." placeholder="Kreuzberg, Berlin" /></div>
</Story>

<Story name="With an error" asChild>
	<div class="max-w-90"><TextField id="budget" label="Budget" value="four hundred" error="Enter an amount, like 400 or 400–800." /></div>
</Story>

<Story name="Select" asChild>
	<div class="max-w-90"><SelectField id="slot" label="Slot" options={slots} value="peak" /></div>
</Story>

<Story name="Search form" asChild>
	<form role="search" aria-label="Find DJs" class="flex max-w-180 flex-wrap items-end gap-3" onsubmit={(event) => event.preventDefault()}>
		<div class="min-w-0 flex-[2_1_14rem]"><TextField id="form-sound" label="Sound or name" placeholder="disco, jungle, a name" /></div>
		<div class="min-w-0 flex-[1_1_10rem]"><TextField id="form-near" label="Near" placeholder="City" /></div>
		<Button type="submit">Find DJs</Button>
	</form>
</Story>
