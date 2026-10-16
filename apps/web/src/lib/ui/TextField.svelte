<script lang="ts">
	import type { HTMLInputAttributes } from 'svelte/elements';
	import Field, { control } from './Field.svelte';

	let {
		id,
		label,
		hint,
		error,
		value = $bindable(''),
		...rest
	}: { id: string; label: string; hint?: string; error?: string; value?: string } & Omit<HTMLInputAttributes, 'id' | 'value' | 'class'> = $props();
</script>

<Field {id} {label} {hint} {error}>
	{#snippet children({ describedby, invalid })}
		<input {id} bind:value aria-describedby={describedby} aria-invalid={invalid || undefined} class={control} {...rest} />
	{/snippet}
</Field>
