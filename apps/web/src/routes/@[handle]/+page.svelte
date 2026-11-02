<script lang="ts">
	import { Clock, ExternalLink, MapPin } from '@lucide/svelte';
	import Avatar from '#lib/components/Avatar.svelte';
	import DjSets from '#lib/components/DjSets.svelte';
	import DjStats from '#lib/components/DjStats.svelte';
	import ManageLink from '#lib/components/ManageLink.svelte';
	import MixList from '#lib/components/MixList.svelte';
	import PerformerFacts, { performerFacts } from '#lib/components/PerformerFacts.svelte';
	import { accountLabel, placeLabel } from '#lib/format.ts';
	import { LINK_KINDS, labelFor } from '#lib/taxonomy/options.ts';
	import Tag from '#lib/ui/Tag.svelte';
	import { webHref } from '#lib/web-url.ts';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();

	// An account without a profile record still has this page, with only its name and what it plays in.
	const basics = $derived(data.profile?.record.basics);

	const performer = $derived(data.profile?.record.performer);

	const promoter = $derived(data.profile?.record.promoter);

	const place = $derived(basics?.location && placeLabel({ location: basics.location }));
</script>

<div class="border-b border-line">
	<div class="container-page flex flex-col gap-6 py-10 sm:py-14">
		<div class="flex flex-col gap-5 sm:flex-row sm:items-end">
			<Avatar src={data.profile?.avatarUrl} name={data.name} size="xl" />
			<div class="flex min-w-0 flex-1 flex-col gap-2">
				<h1 class="m-0 text-title font-semibold">{data.name}</h1>
				{#if performer?.tagline}<p class="m-0 text-lede text-fg-soft sm:text-intro">{performer.tagline}</p>{/if}
				<p class="m-0 flex flex-wrap items-center gap-x-4 gap-y-1 text-small text-fg-muted">
					<span class="break-all">{accountLabel(data.account)}</span>
					{#if place}<span class="inline-flex items-center gap-1.5"><MapPin class="size-4" />{place}</span>{/if}
					{#if basics?.pronouns}<span>{basics.pronouns}</span>{/if}
					{#if performer?.activeSince}<span class="inline-flex items-center gap-1.5"><Clock class="size-4" />Playing since {performer.activeSince}</span>{/if}
				</p>
				{#if !data.profile}<p class="m-0 text-small text-fg-muted">Hasn't set up a profile yet.</p>{/if}
			</div>
		</div>
		{#if performer?.genres?.length}
			<ul class="m-0 flex list-none flex-wrap gap-1.5 p-0" aria-label="Sound">
				{#each performer.genres as genre, index (index)}<li><Tag href="/projects?genres={encodeURIComponent(genre)}">{genre}</Tag></li>{/each}
			</ul>
		{/if}
		<div class="flex flex-wrap gap-2 empty:hidden"><ManageLink viewer={data.viewer} profile={data.account} /></div>
	</div>
</div>

<div class="container-page flex flex-wrap gap-x-10 gap-y-8 py-10">
	<div class="flex min-w-0 flex-[999_1_34rem] flex-col gap-10">
		{#if basics?.bio || performer?.lookingFor}
			<section aria-labelledby="about" class="flex flex-col gap-3">
				<h2 id="about" class="m-0 text-heading font-semibold">About</h2>
				{#if basics?.bio}<p class="m-0 max-w-[65ch] whitespace-pre-line text-fg-soft">{basics.bio}</p>{/if}
				{#if performer?.lookingFor}
					<div class="flex flex-col gap-1">
						<h3 class="m-0 eyebrow text-fg-subtle">Looking for</h3>
						<p class="m-0 max-w-[65ch] whitespace-pre-line text-small text-fg-muted">{performer.lookingFor}</p>
					</div>
				{/if}
			</section>
		{/if}

		{#if performer?.mixes?.length}
			<section aria-labelledby="mixes" class="flex flex-col gap-4">
				<h2 id="mixes" class="m-0 text-heading font-semibold">Mixes</h2>
				<MixList mixes={performer.mixes} sets={data.sets} />
			</section>
		{/if}

		{#await data.dj}
			<div aria-busy="true" aria-label="Loading how they play" class="flex flex-col gap-4">
				<div class="h-7 w-48 animate-pulse rounded-md bg-raised"></div>
				<div class="h-28 animate-pulse rounded-xl bg-surface"></div>
			</div>
		{:then accounts}
			{#each accounts as account (account.did)}
				{#if account.profile}
					<section aria-labelledby="stats" class="flex flex-col gap-4">
						<h2 id="stats" class="m-0 text-heading font-semibold">How they play</h2>
						<DjStats profile={account.profile} />
					</section>
				{/if}
				{#if account.sets.length}
					<section aria-labelledby="sets" class="flex flex-col gap-4">
						<h2 id="sets" class="m-0 text-heading font-semibold">Recent sets</h2>
						<DjSets sets={account.sets} />
					</section>
				{/if}
			{/each}
		{/await}

		{#if performer?.highlights?.length}
			<section aria-labelledby="highlights" class="flex flex-col gap-3">
				<h2 id="highlights" class="m-0 text-heading font-semibold">Highlights</h2>
				<ul class="m-0 flex list-none flex-col gap-2 p-0">
					{#each performer.highlights as highlight, index (index)}<li class="border-l-2 border-line-strong pl-3 text-fg-soft">{highlight}</li>{/each}
				</ul>
			</section>
		{/if}
	</div>

	<aside class="flex min-w-0 flex-[1_1_18rem] flex-col gap-6">
		{#if performer && performerFacts(performer).length}
			<section aria-labelledby="glance" class="card flex flex-col gap-4 p-5">
				<h2 id="glance" class="m-0 eyebrow text-fg-subtle">At a glance</h2>
				<PerformerFacts {performer} />
			</section>
		{/if}
		{#if promoter?.cities?.length}
			<section aria-labelledby="books-in" class="card flex flex-col gap-3 p-5 text-small">
				<h2 id="books-in" class="m-0 eyebrow text-fg-subtle">Books shows in</h2>
				<ul class="m-0 flex list-none flex-col gap-1.5 p-0">
					{#each promoter.cities as city, index (index)}<li>{placeLabel({ location: city })}</li>{/each}
				</ul>
			</section>
		{/if}
		{#if basics?.links?.length}
			<section aria-labelledby="links" class="card flex flex-col gap-3 p-5">
				<h2 id="links" class="m-0 eyebrow text-fg-subtle">Links</h2>
				<ul class="m-0 flex list-none flex-col gap-2 p-0 text-small">
					{#each basics.links as link, index (index)}
						<li><a class="link inline-flex items-center gap-1.5" href={webHref(link.url)} rel="noopener noreferrer nofollow" target="_blank">{link.label ?? labelFor(LINK_KINDS, link.kind)}<ExternalLink class="size-3 opacity-70" /></a></li>
					{/each}
				</ul>
			</section>
		{/if}
		{#await data.playsIn then projects}
			{#if projects?.length}
				<section aria-labelledby="plays-in" class="card flex flex-col gap-3 p-5 text-small">
					<h2 id="plays-in" class="m-0 eyebrow text-fg-subtle">Plays in</h2>
					<ul class="m-0 flex list-none flex-col gap-1.5 p-0">
						{#each projects as project (project.uri)}<li><a class="link" href="/projects/{project.slug}">{project.profile.name}</a></li>{/each}
					</ul>
				</section>
			{/if}
		{/await}
		{#if data.profile}
			<section aria-labelledby="record" class="flex flex-col gap-1.5 px-1 text-caption text-fg-subtle">
				<h2 id="record" class="m-0 eyebrow">On the AT Protocol</h2>
				<p class="m-0 font-mono break-all">{data.profile.uri}</p>
			</section>
		{/if}
	</aside>
</div>
