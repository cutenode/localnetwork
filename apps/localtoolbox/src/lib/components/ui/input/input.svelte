<script lang="ts">
	import type { WithElementRef } from "bits-ui";
	import type { HTMLInputAttributes, HTMLInputTypeAttribute } from "svelte/elements";
	import { cn } from "$lib/utils.ts";

	type InputType = Exclude<HTMLInputTypeAttribute, "file">;

	type Props = WithElementRef<
		Omit<HTMLInputAttributes, "type"> &
			({ type: "file"; files?: FileList } | { type?: InputType; files?: undefined })
	>;

	let {
		ref = $bindable(null),
		value = $bindable(),
		type,
		files = $bindable(),
		class: className,
		"data-slot": dataSlot = "input",
		...restProps
	}: Props = $props();
</script>

{#if type === "file"}
	<input
		bind:this={ref}
		data-slot={dataSlot}
		class={cn(
			"field h-8 w-full min-w-0 py-1 file:inline-flex file:h-6 file:border-0 file:bg-transparent file:text-label file:font-medium file:text-fg",
			className
		)}
		type="file"
		bind:files
		bind:value
		{...restProps}
	/>
{:else}
	<input
		bind:this={ref}
		data-slot={dataSlot}
		class={cn(
			"field h-8 w-full min-w-0 py-1 file:inline-flex file:h-6 file:border-0 file:bg-transparent file:text-label file:font-medium file:text-fg",
			className
		)}
		{type}
		bind:value
		{...restProps}
	/>
{/if}