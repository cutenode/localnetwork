<script lang="ts">
	import EmptyState from '#lib/components/EmptyState.svelte';
	import NeedsList from '#lib/components/NeedsList.svelte';
	import NightList from '#lib/components/NightList.svelte';
	import OnboardingChecklist from '#lib/components/OnboardingChecklist.svelte';
	import { nightLabel, pluralize } from '#lib/format.ts';
	import { statusLine } from '#lib/home.ts';
	import { resolved } from '#lib/resolved.svelte.ts';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();

	const NEEDS_SHOWN = 5;

	const rows = resolved(() => data.needs, null);

	const access = resolved(() => data.access, []);

	const firstRun = resolved(() => data.firstRun, null);

	const summary = resolved(() => data.summary, null);

	const onboarding = resolved(() => data.onboarding, null);

	const needs = $derived(rows.value && [...rows.value, ...access.value]);

	// Known before the rows are, so the heading and the room they take never wait.
	const count = $derived(data.waiting + access.value.length);
</script>

<svelte:head><title>Home · Local Network</title></svelte:head>

{#snippet failure()}
	<p class="notice notice-info m-0 flex flex-wrap items-center justify-between gap-x-4 gap-y-2">Some of your nights didn't load. Everything else here is up to date.<a class="btn-secondary btn-sm" href="/dashboard" data-sveltekit-reload>Try again</a></p>
{/snippet}

<div class="flex max-w-3xl flex-col gap-8 sm:gap-10">
	<header class="flex flex-col gap-1">
		<h1 class="m-0 text-heading font-semibold">{firstRun.value && !count ? 'Set up your page' : statusLine(count)}</h1>
		<!-- Always a line, so the lists below stay put when it arrives. -->
		<p class="m-0 min-h-lh text-body text-fg-muted">
			{#if firstRun.value && !count}
				This goes away when you're done.
			{:else}
				{#await data.nights}
					<span class="sr-only">Finding your next night</span>
				{:then { nights, failed }}
					{@const [next] = nights}
					{#if next}Your next night is {next.when?.toLowerCase() ?? nightLabel(next.date)}: {next.title}.{:else if !failed}No nights coming up.{/if}
				{:catch}
					<!-- The notice below says so. -->
				{/await}
			{/if}
		</p>
	</header>

	{#if onboarding.value}<OnboardingChecklist data={onboarding.value} flags={data.flags} />{/if}

	{#if !data.viewer?.spacesCapable}
		<p class="notice notice-info m-0">Your account's server can't keep private data for Local Network yet, so Local Network keeps your bookings and review requests for you (<a href="/dashboard/data">see what it keeps</a>) and the planner is off. Once it can, sign out and back in.</p>
	{/if}

	{#if !needs && count}
		<section aria-labelledby="needs" class="flex flex-col gap-3">
			<h2 id="needs" class="m-0 text-emphasis font-semibold">Needs you</h2>
			<div role="status" aria-label="Loading what needs you"><NeedsList loading={Math.min(count, NEEDS_SHOWN)} /></div>
		</section>
	{:else if needs?.length}
		<section aria-labelledby="needs" class="flex flex-col gap-3">
			<h2 id="needs" class="m-0 text-emphasis font-semibold">Needs you</h2>
			<NeedsList rows={needs.slice(0, NEEDS_SHOWN)} />
			{#if needs.length > NEEDS_SHOWN}<p class="m-0 text-small text-fg-muted"><a class="link" href="/dashboard/bookings">{needs.length - NEEDS_SHOWN} more</a></p>{/if}
		</section>
	{/if}

	{#await data.nights}
		<section aria-labelledby="next" class="flex flex-col gap-3">
			<h2 id="next" class="m-0 text-emphasis font-semibold">Next up</h2>
			<div role="status" aria-label="Loading your nights"><NightList loading /></div>
		</section>
	{:then { nights, more, failed }}
		{#if nights.length}
			<section aria-labelledby="next" class="flex flex-col gap-3">
				<h2 id="next" class="m-0 text-emphasis font-semibold">Next up</h2>
				<NightList {nights} />
				{#if more}<p class="m-0 text-small text-fg-muted">And {pluralize(more, 'more night')} after that.</p>{/if}
			</section>
		{/if}
		{#if failed}{@render failure()}{/if}
	{:catch}
		{@render failure()}
	{/await}

	{#if summary.value?.others}<p class="m-0 text-small text-fg-muted">{summary.value.others}</p>{/if}

	{#if firstRun.value && !count}
		<p class="m-0 text-small text-fg-muted">Bookings, shows and anything else show up here when they need you.</p>
	{:else if summary.value && !summary.value.hasBookings && !count}
		{#await data.projects then projects}
			{@const [act] = projects ?? []}
			{#if act}
				<p class="m-0 text-small text-fg-muted">All quiet. A good time to freshen up <a class="link" href="/dashboard/projects/{act.slug}">{act.profile.name}</a>.</p>
			{:else if !firstRun.value}
				<EmptyState title="Nothing here yet" body="Bookings show up here as soon as someone writes.">
					<div class="flex flex-wrap justify-center gap-2"><a class="btn-primary" href="/projects">Find DJs</a><a class="btn-secondary" href="/dashboard/projects/new">List a project</a></div>
				</EmptyState>
			{/if}
		{/await}
	{/if}
</div>
