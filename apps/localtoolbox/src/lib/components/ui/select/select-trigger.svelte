<script lang="ts">
	import { Select as SelectPrimitive, type WithoutChild } from "bits-ui";
	import ChevronDownIcon from '@lucide/svelte/icons/chevron-down';
	import { cn } from "$lib/utils.ts";

	let {
		ref = $bindable(null),
		class: className,
		children,
		size = "default",
		...restProps
	}: WithoutChild<SelectPrimitive.TriggerProps> & { size?: "sm" | "default" } = $props();
</script>

<SelectPrimitive.Trigger
	bind:ref
	data-slot="select-trigger"
	data-size={size}
	class={cn(
		"field gap-1.5 pr-2 select-none hover:border-fg-subtle data-placeholder:text-fg-subtle data-[size=default]:h-8 data-[size=sm]:h-7 data-[size=sm]:rounded-sm *:data-[slot=select-value]:flex *:data-[slot=select-value]:gap-1.5 [&_svg:not([class*='size-'])]:size-4 flex w-fit items-center justify-between whitespace-nowrap *:data-[slot=select-value]:line-clamp-1 *:data-[slot=select-value]:items-center [&_svg]:pointer-events-none [&_svg]:shrink-0",
		className
	)}
	{...restProps}
>
	{@render children?.()}
	<ChevronDownIcon class="size-4 text-fg-muted pointer-events-none" />
</SelectPrimitive.Trigger>