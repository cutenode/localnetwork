<script lang="ts">
	import type { Combobox } from '#lib/combobox.svelte.ts';
	import type { Place } from '#lib/places.ts';

	/** The places a place search suggests, as the listbox its input points at, and a line telling screen readers how many. */
	let { combobox, id, class: extra = 'z-20' }: { combobox: Combobox<Place>; id: string; class?: string } = $props();
</script>

<ul {id} role="listbox" aria-label="Places" hidden={!combobox.open} class="absolute inset-x-0 top-full m-0 mt-1 max-h-64 list-none overflow-auto rounded-lg border border-line-strong bg-raised p-1 text-small {extra}">
	{#each combobox.items as place, i (place.label + place.latE6)}
		<!-- The input keeps focus and handles the keys (aria-activedescendant), so the options only take the pointer. -->
		<!-- svelte-ignore a11y_click_events_have_key_events -->
		<li
			id="{id}-{i}"
			role="option"
			aria-selected={i === combobox.active}
			class="cursor-pointer rounded-md px-3 py-2 text-fg-soft hover:text-fg aria-selected:bg-well aria-selected:text-fg"
			onpointerdown={(event) => event.preventDefault()}
			onpointermove={() => (combobox.active = i)}
			onclick={() => combobox.choose(place)}
		>
			{place.label}
		</li>
	{/each}
</ul>
<p class="sr-only" role="status">{combobox.open ? `${combobox.items.length} ${combobox.items.length === 1 ? 'place' : 'places'} found. Use the arrow keys to pick one.` : ''}</p>
