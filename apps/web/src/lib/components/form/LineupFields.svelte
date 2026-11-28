<script lang="ts">
	import { Plus } from '@lucide/svelte';
	import type { LineupEntry } from '#lib/events.ts';
	import RemoveRow from './RemoveRow.svelte';

	/**
	 * An event's lineup as rows of who plays and when: a profile's handle or an act's page link (`performer`), and the
	 * set's start and end on the venue's clock (`setStart`, `setEnd`), which parseEventForm reads, up to the 50 an event
	 * holds. With none saved it starts with one empty row. `error` is why a save refused someone on it.
	 */
	let { saved = [], error, readonly = false }: { saved?: LineupEntry[]; error?: string; readonly?: boolean } = $props();

	const id = $props.id();

	const blank = () => ({ value: '', start: '', end: '' });

	const initialRows = () => (saved.length ? saved.map(({ value, start, end }) => ({ value, start: start ?? '', end: end ?? '' })) : [blank()]);

	let rows = $state(initialRows());
</script>

<div class="flex flex-col gap-2">
	{#each rows as row, index (index)}
		<!-- On a phone the set times and the remove button take their own row under who's playing. -->
		<div class="flex flex-col gap-2 sm:flex-row">
			<input class="input sm:min-w-0 sm:flex-[1_1_12rem]" name="performer" bind:value={row.value} placeholder="@handle or act link" aria-label="Performer" {readonly} aria-invalid={error ? 'true' : undefined} aria-describedby={error ? `${id}-error` : undefined} />
			<div class="flex gap-2">
				<input class="input w-28 min-w-0 flex-1 sm:flex-none" type="time" name="setStart" bind:value={row.start} aria-label="Set starts" {readonly} />
				<input class="input w-28 min-w-0 flex-1 sm:flex-none" type="time" name="setEnd" bind:value={row.end} aria-label="Set ends" {readonly} />
				<RemoveRow label="Remove from lineup" onclick={() => (rows = rows.filter((_, i) => i !== index))} />
			</div>
		</div>
	{/each}
	{#if error}<p id="{id}-error" class="text-small text-critical">{error}</p>{/if}
	{#if rows.length < 50}
		<button type="button" class="btn-ghost btn-sm self-start" onclick={() => (rows = [...rows, blank()])}><Plus class="size-4" />Add to lineup</button>
	{/if}
</div>
