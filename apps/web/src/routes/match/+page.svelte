<script lang="ts">
	import { CircleAlert, CircleCheck, Sparkles } from '@lucide/svelte';
	import { enhance } from '$app/forms';
	import { page } from '$app/state';
	import Avatar from '#lib/components/Avatar.svelte';
	import EmptyState from '#lib/components/EmptyState.svelte';
	import FindDjsSwitch from '#lib/components/FindDjsSwitch.svelte';
	import FormMessage from '#lib/components/FormMessage.svelte';
	import GenreInput from '#lib/components/GenreInput.svelte';
	import NearInput from '#lib/components/NearInput.svelte';
	import ScoreBar from '#lib/components/ScoreBar.svelte';
	import { feeSummary, kindLabel, placeLabel } from '#lib/format.ts';
	import type { MatchBreakdown } from '#lib/matching.ts';
	import { FEE_TIERS, FORMATS, SET_TYPES } from '#lib/taxonomy/options.ts';
	import Button from '#lib/ui/Button.svelte';
	import Card from '#lib/ui/Card.svelte';
	import SectionHeader from '#lib/ui/SectionHeader.svelte';
	import Tag from '#lib/ui/Tag.svelte';
	import type { PageProps } from './$types';

	let { data, form }: PageProps = $props();

	const c = $derived(data.criteria);

	let selected = $state<string[]>([]);

	/** The directory's factors in the order they weigh on the score. */
	const factors: [keyof MatchBreakdown, string][] = [
		['genre', 'sound'],
		['distance', 'distance'],
		['budget', 'budget'],
		['capacity', 'room'],
		['setType', 'slot'],
		['format', 'setup']
	];
</script>

<div class="container-page flex flex-col gap-8 py-10 sm:gap-10 sm:py-14">
	<div class="flex flex-col gap-6">
		<FindDjsSwitch current="/match" />
		<SectionHeader level={1} eyebrow="For promoters" tone="aqua" title="Who fits your show?" lede="Describe the night. We score every listed DJ on sound (40%), distance (25%), budget (15%), room size, slot and setup, and tell you why." />
	</div>

	<div class="grid items-start gap-6 lg:grid-cols-[22rem_minmax(0,1fr)] lg:gap-10">
		<form method="GET" class="card flex flex-col gap-4 p-5 lg:sticky lg:top-20">
			<input type="hidden" name="go" value="1" />
			<h2 class="m-0 text-emphasis font-semibold">Your show</h2>
			<div>
				<label class="label" for="title">Show name <span class="font-normal text-fg-subtle">(optional)</span></label>
				<input id="title" name="title" class="input" value={c.title} maxlength="120" />
			</div>
			<div class="grid grid-cols-2 gap-3">
				<div class="min-w-0">
					<label class="label" for="date">Date</label>
					<input id="date" type="date" name="date" class="input" value={c.date} />
				</div>
				<div class="min-w-0">
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
				<div class="min-w-0">
					<label class="label" for="budget">Budget</label>
					<select id="budget" name="budget" class="input">
						<option value="">Any</option>
						{#each FEE_TIERS as option (option.value)}<option value={option.value} selected={c.budgetTier === option.value}>{option.label}</option>{/each}
					</select>
				</div>
				<div class="min-w-0">
					<label class="label" for="setType">Slot</label>
					<select id="setType" name="setType" class="input">
						<option value="">Any</option>
						{#each SET_TYPES as option (option.value)}<option value={option.value} selected={c.setType === option.value}>{option.label}</option>{/each}
					</select>
				</div>
			</div>
			<fieldset class="m-0 border-0 p-0">
				<legend class="label">Booth setup</legend>
				<div class="flex flex-wrap gap-1.5">
					{#each FORMATS as option (option.value)}
						<label class="chip-toggle"><input type="checkbox" class="sr-only" name="format" value={option.value} checked={c.formats.includes(option.value)} />{option.label}</label>
					{/each}
				</div>
			</fieldset>
			<button class="btn-primary mt-1"><Sparkles class="size-4" />Find matches</button>
		</form>

		<section aria-labelledby="matches-title" class="flex min-w-0 flex-col gap-4">
			<h2 id="matches-title" class="sr-only">Matches</h2>
			<FormMessage {form} />
			{#if !data.searched}
				<EmptyState title="Fill in what you know" body="Every field is optional. The more you tell us, the sharper the ranking." />
			{:else if !data.results}
				<EmptyState title="Directory offline" body="The DJ directory isn't answering. Try again in a minute.">
					<Button variant="secondary" href={page.url.pathname + page.url.search}>Try again</Button>
				</EmptyState>
			{:else if data.results.length === 0}
				<EmptyState title="No DJs in range" body="Widen the radius or relax the budget. DJs outside your radius still show up if they travel that far." />
			{:else}
				{#if data.viewer?.spacesCapable}
					<form method="POST" action="?{page.url.searchParams}&/saveShow" class="card flex flex-wrap items-center justify-between gap-3 px-5 py-4" use:enhance>
						{#each selected as uri (uri)}<input type="hidden" name="project" value={uri} />{/each}
						<p class="m-0 text-small text-fg-soft">{selected.length ? `${selected.length} selected for the lineup.` : 'Tick DJs to start a lineup in your private planner.'}</p>
						<button class="btn-primary btn-sm" disabled={selected.length === 0}>Save show to planner</button>
					</form>
				{/if}
				<ol class="m-0 flex list-none flex-col gap-3 p-0">
					{#each data.results as result, index (result.candidate.uri)}
						{@const project = result.candidate}
						<Card as="li" radius="2xl" class="flex flex-col gap-4 max-sm:p-4">
							<div class="flex flex-wrap items-start gap-x-8 gap-y-4">
								<div class="flex min-w-0 flex-[1_1_16rem] flex-col gap-3">
									<div class="flex items-center gap-3">
										<span class="w-5 shrink-0 font-mono text-caption text-fg-subtle">{index + 1}</span>
										<Avatar src={project.avatarUrl} name={project.profile.name} />
										<div class="min-w-0 flex-1">
											<a class="text-emphasis font-semibold text-fg no-underline hover:underline" href="/projects/{project.slug}">{project.profile.name}</a>
											<p class="m-0 text-label text-fg-muted">{[kindLabel(project.profile.kind), placeLabel(project.profile), feeSummary(project.profile)].filter(Boolean).join(' · ')}</p>
										</div>
									</div>
									<ul class="m-0 flex list-none flex-wrap gap-x-4 gap-y-1 p-0 text-small text-fg-soft">
										{#each result.reasons as reason (reason)}<li class="inline-flex items-center gap-1.5"><CircleCheck class="size-3.5 text-good" />{reason}</li>{/each}
										{#each result.warnings as warning (warning)}<li class="inline-flex items-center gap-1.5"><CircleAlert class="size-3.5 text-warning" />{warning}</li>{/each}
									</ul>
								</div>
								<div class="flex min-w-0 flex-[1_1_15rem] flex-col gap-2.5">
									<ScoreBar score={result.score} />
									<dl class="m-0 grid grid-cols-[3.75rem_minmax(0,1fr)_3.75rem_minmax(0,1fr)] items-center gap-x-2.5 gap-y-1.5 font-mono text-caption text-fg-subtle">
										{#each factors as [factor, label] (factor)}
											<dt>{label}</dt>
											<dd class="m-0 h-1.5 overflow-hidden rounded-full bg-line"><span class="sr-only">{result.breakdown[factor]} of 100</span><span aria-hidden="true" class="block h-full rounded-full bg-track-aqua" style:width="{result.breakdown[factor]}%"></span></dd>
										{/each}
									</dl>
								</div>
							</div>
							<div class="flex flex-wrap items-center gap-x-4 gap-y-3 border-t border-line pt-4">
								<div class="flex min-w-0 flex-[1_1_16rem] flex-wrap gap-1.5">{#each project.profile.genres.slice(0, 6) as genre, i (i)}<Tag>{genre}</Tag>{/each}</div>
								{#if data.viewer?.spacesCapable}
									<label class="flex items-center gap-2 text-small text-fg-soft"><input type="checkbox" class="checkbox" value={project.uri} bind:group={selected} />Add to lineup</label>
								{/if}
								{#if project.managers.length}<Button size="sm" variant="secondary" href="/dashboard/bookings/new?project={project.slug}{c.date ? `&date=${c.date}` : ''}">Inquire</Button>{/if}
							</div>
						</Card>
					{/each}
				</ol>
			{/if}
		</section>
	</div>
</div>
