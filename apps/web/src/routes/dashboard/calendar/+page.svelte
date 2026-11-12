<script lang="ts">
	import EmptyState from '#lib/components/EmptyState.svelte';
	import NightList from '#lib/components/NightList.svelte';
	import NightsFailed from '#lib/components/NightsFailed.svelte';
	import type { NightRow } from '#lib/home.ts';
	import { resolved } from '#lib/resolved.svelte.ts';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();

	const projects = resolved(() => data.projects, null);

	const sections = resolved(() => data.sections, null);

	const thisYear = new Date().getUTCFullYear();

	/** The month a night falls in, with its year once that's not this one. */
	const monthOf = (date: string) => {
		const day = new Date(`${date}T00:00:00Z`);

		return new Intl.DateTimeFormat('en-GB', { month: 'long', year: day.getUTCFullYear() === thisYear ? undefined : 'numeric', timeZone: 'UTC' }).format(day);
	};

	const byMonth = (nights: NightRow[]) => [...Map.groupBy(nights, (night) => monthOf(night.date))];

	// Someone whose sections aren't known yet might be either, so they get both ways in.
	const isPromoter = $derived(!sections.value || sections.value.promoter);

	const canPlan = $derived(isPromoter && !!data.viewer?.spacesCapable);

	const act = $derived(projects.value?.find((project) => project.status === 'active'));
</script>

<svelte:head><title>Calendar · Local Network</title></svelte:head>

<div class="flex max-w-3xl flex-col gap-8 sm:gap-10">
	<header class="flex flex-col gap-1">
		<h1 class="m-0 text-heading font-semibold">Calendar</h1>
		<p class="m-0 text-body text-fg-muted">Every night you're part of, for the year ahead.</p>
	</header>

	{#await data.nights}
		<!-- Two months' worth, about what a calendar holds, so the footer isn't in view to jump when they arrive. -->
		<div class="flex flex-col gap-8 sm:gap-10" role="status" aria-label="Loading your nights">
			{#each { length: 2 }, month (month)}
				<div class="flex flex-col gap-3">
					<span class="flex h-lh items-center text-emphasis"><span class="h-4 w-28 rounded-md bg-well motion-safe:animate-pulse"></span></span>
					<NightList loading />
				</div>
			{/each}
		</div>
	{:then { nights, failed }}
		{#if failed}<NightsFailed href="/dashboard/calendar" />{/if}
		{#each byMonth(nights) as [month, rows], at (month)}
			<section aria-labelledby="month-{at}" class="flex flex-col gap-3">
				<h2 id="month-{at}" class="m-0 text-emphasis font-semibold">{month}</h2>
				<NightList nights={rows} />
			</section>
		{:else}
			{#if !failed}
				<EmptyState title="No nights coming up" body="Shows you announce, dated bookings and planned nights land here.">
					{#if act || isPromoter}
						<div class="flex flex-wrap justify-center gap-2">
							{#if act}<a class="btn-primary" href="/dashboard/projects/{act.slug}/shows/new">Announce a show</a>{/if}
							{#if canPlan}<a class={act ? 'btn-secondary' : 'btn-primary'} href="/dashboard/planner/shows/new">Plan a show</a>{/if}
							{#if isPromoter && !act}<a class={canPlan ? 'btn-secondary' : 'btn-primary'} href="/projects">Find DJs</a>{/if}
						</div>
					{/if}
				</EmptyState>
			{/if}
		{/each}
	{:catch}
		<NightsFailed href="/dashboard/calendar" />
	{/await}
</div>
