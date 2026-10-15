<script lang="ts">
	import type { HTMLSelectAttributes } from 'svelte/elements';
	import Field, { control } from './Field.svelte';

	let {
		id,
		label,
		hint,
		error,
		options,
		value = $bindable(''),
		...rest
	}: { id: string; label: string; hint?: string; error?: string; options: { value: string; label: string }[]; value?: string } & Omit<HTMLSelectAttributes, 'id' | 'value' | 'class'> =
		$props();
</script>

<Field {id} {label} {hint} {error}>
	{#snippet children({ describedby, invalid })}
		<select {id} bind:value aria-describedby={describedby} aria-invalid={invalid || undefined} class={control} {...rest}>
			{#each options as option (option.value)}<option value={option.value}>{option.label}</option>{/each}
		</select>
	{/snippet}
</Field>
