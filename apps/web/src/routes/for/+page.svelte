<script lang="ts">
	import { ArrowRight } from '@lucide/svelte';
	import { pluralize } from '#lib/format.ts';
	import SideMark, { sides } from '#lib/components/landing/SideMark.svelte';
	import { madeUpBands } from '#lib/mix/wave.ts';
	import Section from '#lib/ui/Section.svelte';
	import SectionHeader from '#lib/ui/SectionHeader.svelte';
	import Waveform from '#lib/ui/Waveform.svelte';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();

	const bands = madeUpBands(160);

	/** A night's running order for the promoters' door: where each slot starts and how much of the night it takes, in eighths. */
	const night = [
		{ time: '22:00', slot: 'Opener', start: 0, span: 3 },
		{ time: '00:00', slot: 'Peak time', start: 3, span: 3 },
		{ time: '02:00', slot: 'Closing', start: 6, span: 2 }
	];

	const doors = [
		{
			side: 'promoters',
			title: 'I put on shows.',
			body: 'Find DJs by sound, distance and budget, and book them in private threads.',
			points: ['Find DJs', 'Match a show', 'Planner']
		},
		{
			side: 'djs',
			title: 'I play out.',
			body: 'Get listed, keep a profile you own, and get booked by promoters near you.',
			points: ['Get listed', 'Press kits', 'Charts']
		}
	] as const;

	/** For people who only want the music. */
	const listening = [
		{ href: '/shows', label: 'Shows' },
		{ href: '/charts', label: 'Charts' },
		{ href: '/djs', label: 'Find DJs' }
	];
</script>

<Section tone="grid" space="md" labelledby="hero-title">
	<div class="flex flex-col items-center gap-5 pt-4 sm:pt-6">
		<SectionHeader id="hero-title" level={1} size="display" align="center" title="Which side of the booth are you on?" lede="Local Network connects DJs with the people who book them." />
		<!-- One line tall from the start, so the totals arriving don't push the doors down. -->
		<p class="m-0 min-h-6 text-small text-fg-subtle tabular-nums">
			{#await data.stats then stats}
				{#if stats?.djs}{pluralize(stats.djs, 'DJ')} in {pluralize(stats.cities, 'city', 'cities')} so far.{/if}
			{/await}
		</p>
	</div>
</Section>

{#snippet running()}
	<div class="flex flex-col gap-2" aria-hidden="true">
		{#each night as slot (slot.time)}
			<div class="grid grid-cols-8 items-center">
				<span
					class="flex h-9 items-center justify-between gap-2 rounded-md border border-line-strong bg-well px-3 font-mono text-caption"
					style:grid-column="{slot.start + 1} / span {slot.span}"
				>
					<span class="text-track-aqua">{slot.time}</span><span class="truncate text-fg-muted max-lg:hidden">{slot.slot}</span>
				</span>
			</div>
		{/each}
	</div>
{/snippet}

{#snippet wave()}
	<Waveform {bands} playhead={0.38} class="h-28 opacity-70 transition-opacity duration-300 group-hover:opacity-100" />
{/snippet}

<Section space="md" labelledby="doors-title">
	<div class="flex flex-col gap-10 sm:gap-14">
		<h2 id="doors-title" class="sr-only">Choose your side</h2>
		<ul class="m-0 grid list-none gap-4 p-0 md:grid-cols-2">
			{#each doors as door (door.side)}
				{@const { href, title: name, tone, transition } = sides[door.side]}
				<!-- The heading is the link and its hit area covers the card, so the door has a short name and one tab stop. -->
				<li class="group relative flex flex-col overflow-hidden rounded-2xl border border-line bg-surface transition-colors duration-300 hover:border-line-strong">
					<div class="flex flex-col gap-3 p-5 sm:p-6">
						<h3 class="m-0 text-title font-semibold">
							<a class="door text-fg no-underline outline-none after:absolute after:inset-0" {href}>{door.title}</a>
						</h3>
						<p class="m-0 max-w-md text-lede text-fg-muted">{door.body}</p>
						<ul class="m-0 mt-1 flex list-none flex-wrap gap-2 p-0" aria-label="What's there">
							{#each door.points as point (point)}<li class="chip">{point}</li>{/each}
						</ul>
					</div>
					<div class="mt-auto px-5 sm:px-6">
						{#if door.side === 'djs'}{@render wave()}{:else}{@render running()}{/if}
					</div>
					<p class="m-0 mt-6 flex items-center justify-between gap-3 border-t border-line bg-canvas px-5 py-4 font-semibold sm:px-6 {tone}">
						<span class="inline-flex items-center gap-2" style:view-transition-name={transition}><SideMark side={door.side} />{name}</span>
						<ArrowRight aria-hidden="true" class="size-5 transition-transform duration-300 ease-out group-hover:translate-x-1 motion-reduce:transition-none" />
					</p>
				</li>
			{/each}
		</ul>
		<nav aria-label="Just here for the music" class="flex flex-wrap items-baseline justify-center gap-x-5 gap-y-2 text-small">
			<span class="text-fg-muted">Just here for the music?</span>
			{#each listening as link (link.href)}<a class="link" href={link.href}>{link.label}</a>{/each}
		</nav>
	</div>
</Section>

<style>
	/* The door's ring goes round the whole card its link covers. */
	.door:focus-visible::after {
		border-radius: var(--radius-2xl);
		outline: 2px solid var(--color-link);
		outline-offset: 2px;
	}
</style>
