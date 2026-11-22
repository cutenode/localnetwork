<script lang="ts" generics="T">
	import type { Combobox } from '#lib/combobox.page-only.svelte.ts';
	import { pluralize } from '#lib/format.ts';

	/**
	 * A combobox's suggestions, as the listbox its input points at, and a line telling screen readers how many. `text`
	 * is what each shows, `detail` an optional second line under it, `key` tells them apart, and `noun` names one in
	 * that line.
	 */
	let {
		combobox,
		id,
		label,
		noun,
		text,
		detail,
		key,
		class: extra = 'z-20'
	}: {
		combobox: Combobox<T>;
		id: string;
		label: string;
		noun: string;
		text: (item: T) => string;
		detail?: (item: T) => string;
		key: (item: T) => string;
		class?: string;
	} = $props();
</script>

<ul {id} role="listbox" aria-label={label} hidden={!combobox.open} class="absolute inset-x-0 top-full m-0 mt-1 max-h-64 list-none overflow-auto rounded-lg border border-line-strong bg-raised p-1 text-small {extra}">
	{#each combobox.items as item, i (key(item))}
		<!-- The input keeps focus and handles the keys (aria-activedescendant), so the options only take the pointer. -->
		<!-- svelte-ignore a11y_click_events_have_key_events -->
		<li
			id="{id}-{i}"
			role="option"
			aria-selected={i === combobox.active}
			class="cursor-pointer rounded-md px-3 py-2 text-fg-soft hover:text-fg aria-selected:bg-well aria-selected:text-fg"
			onpointerdown={(event) => event.preventDefault()}
			onpointermove={() => (combobox.active = i)}
			onclick={() => combobox.choose(item)}
		>
			{text(item)}
			{#if detail}<span class="block text-fg-muted">{detail(item)}</span>{/if}
		</li>
	{/each}
</ul>
<p class="sr-only" role="status">{combobox.open ? `${pluralize(combobox.items.length, noun)} found. Use the arrow keys to pick one.` : ''}</p>
