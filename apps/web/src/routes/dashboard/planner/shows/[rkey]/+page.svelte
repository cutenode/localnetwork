<script lang="ts">
	import { ArrowLeft, Mail, Plus, Trash2 } from '@lucide/svelte';
	import FormMessage from '#lib/components/FormMessage.svelte';
	import GenreInput from '#lib/components/GenreInput.svelte';
	import { FEE_TIERS, SET_TYPES } from '#lib/taxonomy/options.ts';
	import Card from '#lib/ui/Card.svelte';
	import Eyebrow from '#lib/ui/Eyebrow.svelte';
	import Monogram from '#lib/ui/Monogram.svelte';
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

<div class="flex max-w-3xl flex-col gap-8">
	<a href="/dashboard/planner" class="inline-flex items-center gap-1.5 self-start text-small text-fg-muted no-underline hover:text-fg"><ArrowLeft class="size-4" />Planner</a>
	<header class="flex flex-col gap-3">
		<Eyebrow>{data.isNew ? 'New show' : 'Show'}</Eyebrow>
		<h1 class="m-0 text-title font-semibold">{data.isNew ? 'Plan a show' : details?.title}</h1>
	</header>
	<form method="POST" class="flex flex-col gap-6">
		<FormMessage {form} />
		<Card as="section" class="flex flex-col gap-4 sm:p-6">
			<h2 class="m-0 text-emphasis font-semibold">Details</h2>
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
			<div><label class="label" for="notes">Notes</label><textarea id="notes" name="notes" class="input min-h-24 py-2.5" maxlength="2000">{v.notes ?? ''}</textarea></div>
		</Card>
		<Card as="section" class="flex flex-col gap-4 sm:p-6">
			<h2 class="m-0 text-emphasis font-semibold">Lineup</h2>
			{#if lineup.length}
				<ol class="m-0 flex list-none flex-col gap-2 p-0">
					{#each lineup as slot, index (slot.project)}
						{@const project = projects[slot.project]}
						{@const name = project?.profile.name ?? 'Removed project'}
						<!-- A saved show's details go into an inquiry to anyone on it who can be contacted. -->
						{@const inquire = data.show && project?.managers.length ? `/dashboard/bookings/new?project=${project.slug}&show=${data.show.rkey}` : null}
						<li class="flex flex-wrap items-center gap-2 rounded-lg border border-line bg-well p-2.5 sm:grid sm:grid-cols-[minmax(0,1fr)_9rem_10rem_auto]">
							<input type="hidden" name="slotProject" value={slot.project} />
							<div class="flex min-w-0 basis-[calc(100%-7rem)] items-center gap-2.5">
								<Monogram {name} size="sm" />
								<a class="truncate text-fg no-underline hover:text-link" href="/projects/{project?.slug}">{name}</a>
							</div>
							<div class="ml-auto flex sm:order-last">
								{#if inquire}<a class="btn-ghost px-2.5" href={inquire} aria-label="Inquire with {name}" title="Inquire"><Mail class="size-4" /></a>{/if}
								<button type="button" class="btn-ghost px-2.5" aria-label="Remove {name}" onclick={() => (lineup = lineup.filter((_, i) => i !== index))}><Trash2 class="size-4" /></button>
							</div>
							<input name="slotLabel" class="input min-w-0 grow basis-[calc(50%-0.25rem)]" placeholder="e.g. 23:00–01:00" value={slot.slot ?? ''} aria-label="Slot" />
							<select name="slotStatus" class="input min-w-0 grow basis-[calc(50%-0.25rem)]" value={slot.status ?? 'considering'} aria-label="Status">
								<option value="considering">Considering</option><option value="contacted">Contacted</option><option value="confirmed">Confirmed</option>
							</select>
						</li>
					{/each}
				</ol>
			{:else}
				<p class="m-0 text-small text-fg-muted">Add projects from your saved list.</p>
			{/if}
			{#if addable.length}
				<div class="flex gap-2">
					<select class="input min-w-0 flex-1" bind:value={adding} aria-label="Add a saved project">
						<option value="">Add a saved project…</option>
						{#each addable as save (save.rkey)}<option value={save.project}>{projects[save.project]?.profile.name ?? save.project}</option>{/each}
					</select>
					<button type="button" class="btn-secondary" disabled={!adding} onclick={() => { lineup = [...lineup, { project: adding, status: 'considering' }]; adding = ''; }}><Plus class="size-4" />Add</button>
				</div>
			{/if}
		</Card>
		<div class="flex sm:justify-end"><button class="btn-primary w-full sm:w-auto">Save show</button></div>
	</form>
</div>
