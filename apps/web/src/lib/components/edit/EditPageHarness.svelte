<script lang="ts">
	import type { ComponentProps } from 'svelte';
	import AddSlot from './AddSlot.svelte';
	import EditMode from './EditMode.svelte';
	import InlineText from './InlineText.svelte';

	/**
	 * A small act page in edit mode, for its tests and stories: a name, a tagline and a notice only its people see, and
	 * an empty links section that edit mode draws as a slot. `picture` puts a picture's controls before the name, as a
	 * page with one has them.
	 */
	let {
		saved = { name: 'Halflight', tagline: 'Deep, patient house for small rooms' },
		picture = false,
		...edit
	}: Omit<ComponentProps<typeof EditMode>, 'children' | 'name' | 'version' | 'labels'> & { saved?: { name: string; tagline: string }; picture?: boolean } = $props();
</script>

<EditMode {...edit} name={saved.name} version="bafyreiopened" labels={{ name: 'Name', tagline: 'Tagline' }}>
	{#snippet children(view)}
		<article class="container-page flex flex-col gap-4 py-8">
			{#if view.editing}
				{#if picture}
					<label class="label">Picture <input type="file" name="avatar" accept="image/*" /></label>
					<label class="label"><input type="checkbox" class="checkbox" name="removeAvatar" />Remove</label>
				{/if}
				<h1 class="m-0 text-display font-semibold"><InlineText class="text-display" name="name" label="Name" required value={view.value('name', saved.name)} error={view.error('name')} readonly={view.busy} /></h1>
				<p class="m-0 text-emphasis text-fg-soft"><InlineText class="text-emphasis" name="tagline" label="Tagline" placeholder="A line about the act" value={view.value('tagline', saved.tagline)} error={view.error('tagline')} readonly={view.busy} /></p>
				<AddSlot label="Add links" hint="Where to hear them and book them" />
			{:else}
				<h1 class="m-0 text-display font-semibold">{saved.name}</h1>
				<p class="m-0 text-emphasis text-fg-soft">{saved.tagline}</p>
				{#if !view.visitor && edit.role}<p class="m-0 notice notice-info">Bookings and press kits are in its dashboard.</p>{/if}
			{/if}
		</article>
	{/snippet}
</EditMode>
