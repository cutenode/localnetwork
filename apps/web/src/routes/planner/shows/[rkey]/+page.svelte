<script lang="ts">
	import { Plus, Trash2 } from '@lucide/svelte';
	import FormMessage from '#lib/components/FormMessage.svelte';
	import GenreInput from '#lib/components/GenreInput.svelte';
	import { FEE_TIERS, SET_TYPES } from '#lib/taxonomy/options.ts';
	import type { PageProps } from './$types';

	type Slot = { project: string; slot?: string; status?: string };

	let { data, form }: PageProps = $props();

	const projects = $derived(data.projects);

	const details = $derived(data.show?.details);

	// After a failed save, the form shows what was typed instead of the saved show.
	// SAFETY: the save action echoes `{ ...stringValues(form), lineup }`, so every field but `lineup` is submitted text.
	const echoed = $derived(form?.values as Record<string, string> | undefined);

	const v = $derived(
		echoed ?? {
			title: details?.title ?? '',
			date: details?.date?.slice(0, 10) ?? '',
			venue: details?.venue ?? '',
			capacity: details?.capacity?.toString() ?? '',
			budgetTier: details?.budgetTier ?? '',
			setType: details?.setType ?? '',
			genres: details?.genres?.join(', ') ?? '',
			notes: details?.notes ?? ''
		}
	);

	let lineup = $derived<Slot[]>((form?.values?.lineup ?? data.show?.lineup ?? []).map((slot) => ({ ...slot })));

	let adding = $state('');

	const addable = $derived(data.saves.filter((save) => !lineup.some((slot) => slot.project === save.project)));
</script>

<svelte:head><title>{details?.title ?? 'New show'} · Planner</title></svelte:head>

<div class="container-page max-w-3xl py-10">
	<a href="/planner" class="link text-sm">← Planner</a>
	<h1 class="mt-3 text-3xl">{data.isNew ? 'Plan a show' : details?.title}</h1>
	<form method="POST" class="mt-8 space-y-6">
		<FormMessage {form} />
		<section class="card card-pad space-y-4">
			<div><label class="label" for="title">Show name</label><input id="title" name="title" class="input" required maxlength="120" value={v.title ?? ''} /></div>
			<div class="grid gap-4 sm:grid-cols-3">
				<div><label class="label" for="date">Date</label><input id="date" type="date" name="date" class="input" value={v.date ?? ''} /></div>
				<div><label class="label" for="venue">Venue</label><input id="venue" name="venue" class="input" maxlength="120" value={v.venue ?? ''} /></div>
				<div><label class="label" for="capacity">Capacity</label><input id="capacity" type="number" name="capacity" min="1" class="input" value={v.capacity ?? ''} /></div>
			</div>
			<div class="grid gap-4 sm:grid-cols-2">
				<div><label class="label" for="budgetTier">Budget</label><select id="budgetTier" name="budgetTier" class="input"><option value="">Not set</option>{#each FEE_TIERS as o (o.value)}<option value={o.value} selected={v.budgetTier === o.value}>{o.label}</option>{/each}</select></div>
				<div><label class="label" for="setType">Main slot</label><select id="setType" name="setType" class="input"><option value="">Not set</option>{#each SET_TYPES as o (o.value)}<option value={o.value} selected={v.setType === o.value}>{o.label}</option>{/each}</select></div>
			</div>
			<div><label class="label" for="genres">Sound</label><GenreInput value={v.genres ?? ''} /></div>
			<div><label class="label" for="notes">Notes</label><textarea id="notes" name="notes" class="input min-h-24" maxlength="2000">{v.notes ?? ''}</textarea></div>
		</section>
		<section class="card card-pad space-y-3">
			<h2 class="section-title">Lineup</h2>
			{#each lineup as slot, index (slot.project)}
				<div class="flex flex-wrap items-center gap-2 border-2 border-ansi-blue p-2">
					<input type="hidden" name="slotProject" value={slot.project} />
					<a class="min-w-40 flex-1 menu-link" href="/projects/{projects[slot.project]?.slug}">{projects[slot.project]?.profile.name ?? 'Removed project'}</a>
					<input name="slotLabel" class="input w-40" placeholder="e.g. 23:00–01:00" value={slot.slot ?? ''} aria-label="Slot" />
					<select name="slotStatus" class="input w-36" value={slot.status ?? 'considering'} aria-label="Status">
						<option value="considering">Considering</option><option value="contacted">Contacted</option><option value="confirmed">Confirmed</option>
					</select>
					<button type="button" class="btn-ghost px-2" aria-label="Remove" onclick={() => (lineup = lineup.filter((_, i) => i !== index))}><Trash2 class="size-4" /></button>
				</div>
			{:else}
				<p class="muted text-sm">Add projects from your saved list.</p>
			{/each}
			{#if addable.length}
				<div class="flex gap-2">
					<select class="input" bind:value={adding} aria-label="Add a saved project">
						<option value="">Add a saved project…</option>
						{#each addable as save (save.rkey)}<option value={save.project}>{projects[save.project]?.profile.name ?? save.project}</option>{/each}
					</select>
					<button type="button" class="btn-secondary" disabled={!adding} onclick={() => { lineup = [...lineup, { project: adding, status: 'considering' }]; adding = ''; }}><Plus class="size-4" />Add</button>
				</div>
			{/if}
		</section>
		<div class="flex justify-end"><button class="btn-accent">Save show</button></div>
	</form>
</div>
