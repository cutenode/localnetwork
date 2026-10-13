<script lang="ts">
	import { CircleAlert, CircleCheck, Sparkles } from '@lucide/svelte';
	import { enhance } from '$app/forms';
	import { page } from '$app/state';
	import Avatar from '#lib/components/Avatar.svelte';
	import EmptyState from '#lib/components/EmptyState.svelte';
	import FormMessage from '#lib/components/FormMessage.svelte';
	import GenreInput from '#lib/components/GenreInput.svelte';
	import NearInput from '#lib/components/NearInput.svelte';
	import ScoreBar from '#lib/components/ScoreBar.svelte';
	import { feeLabel, kindLabel, placeLabel } from '#lib/format.ts';
	import { FEE_TIERS, FORMATS, SET_TYPES } from '#lib/taxonomy/options.ts';
	import type { PageProps } from './$types';

	let { data, form }: PageProps = $props();

	const c = $derived(data.criteria);

	let selected = $state<string[]>([]);
</script>

<svelte:head><title>Match a show · Local Network</title></svelte:head>

<div class="container-page py-10">
	<p class="eyebrow">Matcher</p>
	<h1 class="mt-1 text-4xl">Who fits your show?</h1>
	<p class="muted mt-2 max-w-2xl">Describe the night. We score every listed project on sound (40%), distance (25%), budget (15%), room size, slot, and setup, and tell you why.</p>

	<div class="mt-8 grid gap-8 lg:grid-cols-[22rem_1fr]">
		<form method="GET" class="card card-pad h-fit space-y-4 lg:sticky lg:top-20">
			<input type="hidden" name="go" value="1" />
			<div>
				<label class="label" for="title">Show name <span class="muted">(optional)</span></label>
				<input id="title" name="title" class="input" value={c.title} maxlength="120" />
			</div>
			<div class="grid grid-cols-2 gap-3">
				<div>
					<label class="label" for="date">Date</label>
					<input id="date" type="date" name="date" class="input" value={c.date} />
				</div>
				<div>
					<label class="label" for="capacity">Capacity</label>
					<input id="capacity" type="number" min="1" name="capacity" class="input" value={c.capacity ?? ''} placeholder="300" />
				</div>
			</div>
			<div>
				<label class="label" for="near">Venue location</label>
				<NearInput place={c.place} lat={c.lat} lng={c.lng} geocoder={data.geocoder} />
			</div>
			<div>
				<label class="label" for="radius">Local means within</label>
				<select id="radius" name="radius" class="input">
					{#each [10, 25, 50, 100, 250, 500] as km (km)}<option value={km} selected={c.radiusKm === km}>{km} km</option>{/each}
				</select>
			</div>
			<div>
				<label class="label" for="genres">Sound</label>
				<GenreInput value={c.genres.join(', ')} placeholder="What should the night sound like?" />
			</div>
			<div class="grid grid-cols-2 gap-3">
				<div>
					<label class="label" for="budget">Budget</label>
					<select id="budget" name="budget" class="input">
						<option value="">Any</option>
						{#each FEE_TIERS as option (option.value)}<option value={option.value} selected={c.budgetTier === option.value}>{option.label}</option>{/each}
					</select>
				</div>
				<div>
					<label class="label" for="setType">Slot</label>
					<select id="setType" name="setType" class="input">
						<option value="">Any</option>
						{#each SET_TYPES as option (option.value)}<option value={option.value} selected={c.setType === option.value}>{option.label}</option>{/each}
					</select>
				</div>
			</div>
			<fieldset>
				<legend class="label">Booth setup</legend>
				<div class="flex flex-wrap gap-1.5">
					{#each FORMATS as option (option.value)}
						<label class="chip-toggle"><input type="checkbox" class="sr-only" name="format" value={option.value} checked={c.formats.includes(option.value)} />{option.label}</label>
					{/each}
				</div>
			</fieldset>
			<button class="btn-accent w-full"><Sparkles class="size-4" />Find matches</button>
		</form>

		<section class="space-y-4">
			<FormMessage {form} />
			{#if !data.searched}
				<EmptyState title="Fill in what you know" body="Every field is optional. The more you tell us, the sharper the ranking." />
			{:else if data.results.length === 0}
				<EmptyState title="No projects in range" body="Widen the radius or relax the budget. Projects outside your radius still show up if they'll travel that far." />
			{:else}
				{#if data.viewer?.spacesCapable}
					<form method="POST" action="?{page.url.searchParams}&/saveShow" class="card card-pad flex flex-wrap items-center justify-between gap-3" use:enhance>
						{#each selected as uri (uri)}<input type="hidden" name="project" value={uri} />{/each}
						<p class="text-sm">{selected.length ? `${selected.length} selected for the lineup.` : 'Tick projects to start a lineup in your private planner.'}</p>
						<button class="btn-primary btn-sm" disabled={selected.length === 0}>Save show to planner</button>
					</form>
				{/if}
				<ol class="space-y-4">
					{#each data.results as result, index (result.candidate.uri)}
						{@const project = result.candidate}
						<li class="card card-pad flex flex-col gap-4 sm:flex-row sm:items-start">
							<div class="flex items-start gap-4 sm:w-80">
								<span class="text-2xl text-ansi-yellow">{index + 1}</span>
								<Avatar src={project.avatarUrl} name={project.profile.name} />
								<div class="min-w-0">
									<a class="menu-link text-2xl leading-none uppercase" href="/projects/{project.slug}">{project.profile.name}</a>
									<p class="muted text-sm">{placeLabel(project.profile)}</p>
									<p class="muted text-xs">{kindLabel(project.profile.kind)}{project.profile.fee ? ` · ${feeLabel(project.profile)}` : ''}</p>
								</div>
							</div>
							<div class="flex-1 space-y-2">
								<ScoreBar score={result.score} />
								<ul class="flex flex-wrap gap-x-4 gap-y-1 text-sm">
									{#each result.reasons as reason (reason)}<li class="inline-flex items-center gap-1 text-ansi-bright-green"><CircleCheck class="size-3.5" />{reason}</li>{/each}
									{#each result.warnings as warning (warning)}<li class="inline-flex items-center gap-1 text-ansi-yellow"><CircleAlert class="size-3.5" />{warning}</li>{/each}
								</ul>
								<div class="flex flex-wrap gap-1.5">{#each project.profile.genres.slice(0, 6) as genre, i (i)}<span class="chip">{genre}</span>{/each}</div>
							</div>
							<div class="flex flex-col gap-2 sm:items-end">
								{#if data.viewer?.spacesCapable}
									<label class="flex items-center gap-2 text-sm"><input type="checkbox" class="checkbox" value={project.uri} bind:group={selected} />Lineup</label>
								{/if}
								{#if project.managers.length}<a class="btn-secondary btn-sm" href="/bookings/new?project={project.slug}{c.date ? `&date=${c.date}` : ''}">Inquire</a>{/if}
							</div>
						</li>
					{/each}
				</ol>
			{/if}
		</section>
	</div>
</div>
