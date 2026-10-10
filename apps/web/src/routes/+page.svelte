<script lang="ts">
	import Hotkey from '#lib/components/Hotkey.svelte';
	import NearInput from '#lib/components/NearInput.svelte';
	import { feeLabel, kindLabel } from '#lib/format.ts';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();
</script>

<svelte:head>
	<title>Local Network · Find local DJs for your show</title>
	<meta name="description" content="Promoters find local DJs who fit their show. DJs get a profile they control, built on the AT Protocol." />
</svelte:head>

<div class="container-page flex flex-col gap-8 pt-6">
	<section class="flex flex-col gap-4">
		<p class="eyebrow">For promoters and DJs</p>
		<h1 class="text-[clamp(2.5rem,5.8vw,5rem)] leading-[0.92] text-balance uppercase">Book the right DJ <span class="text-ansi-bright-magenta">from your own scene.</span></h1>
		<p class="max-w-3xl">Search local DJ projects by sound, distance, budget, and the slot you need to fill. DJs keep a profile they control, with booking details kept private.</p>
		<form action="/projects" method="GET" role="search" aria-label="Find DJs" class="card flex max-w-4xl flex-col gap-2.5 border-ansi-magenta px-5 pt-2 pb-5">
			<p class="text-ansi-bright-magenta uppercase">Quick search</p>
			<div class="grid items-center gap-x-3 gap-y-2 sm:grid-cols-[11rem_1fr]">
				<label for="hero-q" class="text-ansi-bright-green">Sound or name:</label>
				<input id="hero-q" name="q" class="input" placeholder="disco, jungle, a name" />
				<label for="hero-near" class="text-ansi-bright-green">Near:</label>
				<NearInput id="hero-near" geocoder={data.geocoder} />
			</div>
			<button class="btn-accent mt-1 self-start">Find DJs</button>
		</form>
	</section>

	<div class="flex flex-wrap gap-6">
		<section class="card min-w-0 flex-[2_1_32rem] border-ansi-bright-blue px-4 pt-2 pb-3">
			<h2 class="section-title mb-2">Sounds on file</h2>
			{#if data.genres.length}
				<ul class="grid grid-cols-[repeat(auto-fill,minmax(min(100%,13rem),1fr))] gap-x-4">
					{#each data.genres as genre (genre.value)}
						<li><a class="menu-link px-0.5" href="/projects?genres={encodeURIComponent(genre.value)}">{genre.value} <span class="text-ansi-yellow">({genre.count})</span></a></li>
					{/each}
				</ul>
			{:else}
				<p class="muted">{data.stats ? 'No projects listed yet.' : "The directory isn't answering. Try again in a minute."}</p>
			{/if}
		</section>
		<section class="card min-w-0 flex-[1_1_16rem] border-ansi-green px-4 pt-2 pb-3">
			<h2 class="section-title mb-2 bg-ansi-green text-ansi-black">System</h2>
			<dl class="grid grid-cols-[1fr_auto] gap-x-4">
				{#if data.stats}
					<dt class="muted">Projects</dt>
					<dd class="text-ansi-white tabular-nums">{data.stats.active}</dd>
					<dt class="muted">Cities</dt>
					<dd class="text-ansi-white tabular-nums">{data.stats.cities}</dd>
					<dt class="muted">Up for claiming</dt>
					<dd class="tabular-nums"><a class="link" href="/projects?unclaimed=on">{data.stats.unclaimed}</a></dd>
				{:else}
					<dt class="muted">Directory</dt>
					<dd class="text-ansi-bright-red" title="The directory isn't answering. Try again in a minute.">Offline</dd>
				{/if}
				<dt class="muted">Changes</dt>
				<dd class="text-ansi-bright-green">Reviewed</dd>
			</dl>
		</section>
	</div>

	{#if data.recent.length}
		<section class="card px-5 pt-3 pb-1">
			<div class="flex flex-wrap items-baseline justify-between gap-x-6 gap-y-1">
				<h2 class="text-4xl uppercase">New files <span class="text-base text-ansi-cyan normal-case">Newly listed projects</span></h2>
				<a class="menu-link px-1" href="/projects?sort=recent">See all</a>
			</div>
			<ol class="mt-2">
				{#each data.recent as project (project.uri)}
					{@const profile = project.profile}
					<li class="border-t-2 border-dashed border-ansi-blue py-3">
						<p class="flex flex-wrap items-baseline gap-x-5">
							<a class="menu-link text-3xl leading-none uppercase" href="/projects/{project.slug}">{profile.name}<span aria-hidden="true" class="text-ansi-cyan">.PRJ</span></a>
							<span class="text-ansi-bright-green">{profile.location.city}, {profile.location.country}</span>
							<span class="text-ansi-yellow">{[kindLabel(profile.kind), profile.fee && feeLabel(profile)].filter(Boolean).join(' · ')}</span>
							{#if project.managers.length === 0}<span class="text-ansi-bright-red uppercase sm:ml-auto" title="Nobody has claimed this project yet">Unclaimed</span>{/if}
						</p>
						{#if profile.tagline}<p class="ml-[2ch] text-ansi-white">{profile.tagline}</p>{/if}
						<p class="ml-[2ch] text-ansi-bright-magenta">{profile.genres.join(' / ')}</p>
					</li>
				{/each}
			</ol>
		</section>
	{/if}

	{#if data.cities.length}
		<section class="card border-ansi-bright-blue px-4 pt-2 pb-3">
			<h2 class="section-title mb-2">Browse by city</h2>
			<ul class="grid grid-cols-[repeat(auto-fill,minmax(min(100%,15rem),1fr))] gap-x-4">
				{#each data.cities as city (city.city + city.country)}
					<li><a class="menu-link px-0.5" href="/projects?q={encodeURIComponent(city.city)}&country={city.country}">{city.city}, {city.country} <span class="text-ansi-yellow">({city.count})</span></a></li>
				{/each}
			</ul>
		</section>
	{/if}

	<section class="card flex flex-wrap items-end gap-x-10 gap-y-4 border-ansi-bright-red px-5 pt-3 pb-5">
		<div class="min-w-0 flex-[1_1_30rem]">
			<p class="text-ansi-bright-red uppercase">Message from the sysop</p>
			<h2 class="mt-1 text-[clamp(2.25rem,4.4vw,3.5rem)] leading-[0.95] uppercase">You play out? Put your project on the map.</h2>
			<p class="mt-2 max-w-2xl">List a solo act, a b2b duo, or a whole crew. Already listed by an admin? Claim it. Your profile is published from a record on the AT Protocol, so it's portable, and your private booking details stay in a members-only space. Every change is reviewed by a moderator.</p>
		</div>
		<nav aria-label="For DJs" class="flex flex-col gap-1 text-xl">
			<a class="menu-link flex items-center gap-2 px-1" href="/projects/new" aria-keyshortcuts="N"><Hotkey key="N" />List a project</a>
			<a class="menu-link flex items-center gap-2 px-1" href="/projects?unclaimed=on" aria-keyshortcuts="C"><Hotkey key="C" />Claim a project</a>
			<a class="menu-link flex items-center gap-2 px-1" href="/for-djs" aria-keyshortcuts="D"><Hotkey key="D" />How it works for DJs</a>
		</nav>
	</section>
</div>
