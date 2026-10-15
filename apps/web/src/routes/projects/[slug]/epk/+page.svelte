<script lang="ts">
	import { ArrowLeft, Check, Copy, ExternalLink, FileText, ImagePlus, Printer, Quote } from '@lucide/svelte';
	import { page } from '$app/state';
	import Avatar from '#lib/components/Avatar.svelte';
	import BookingDetails from '#lib/components/BookingDetails.svelte';
	import DjSets from '#lib/components/DjSets.svelte';
	import DjStats from '#lib/components/DjStats.svelte';
	import ProjectFacts from '#lib/components/ProjectFacts.svelte';
	import { formatDate, kindLabel, placeLabel } from '#lib/format.ts';
	import { LINK_KINDS, labelFor } from '#lib/taxonomy/options.ts';
	import type { PageProps } from './$types';

	/**
	 * A project's electronic press kit, put together from what's on file: the profile, its mixes, the
	 * managers' playing stats and sets, and booking details for members. It reads like a document and
	 * prints on white. Slots for what the site can't hold yet (press photos, quotes, a rider PDF)
	 * show only to the project's managers.
	 */
	let { data }: PageProps = $props();

	const project = $derived(data.project);

	const profile = $derived(project.profile);

	let copied = $state(false);

	async function copyLink() {
		try {
			await navigator.clipboard.writeText(page.url.href);
			copied = true;
			setTimeout(() => (copied = false), 2000);
		} catch {
			copied = false;
		}
	}

	/** What a full press kit has that this one can't hold yet. */
	const missing = [
		{ icon: ImagePlus, title: 'Press photos', body: 'High-resolution photos promoters can use on flyers and listings.' },
		{ icon: Quote, title: 'Press quotes', body: 'What venues, promoters and press have said about your sets.' },
		{ icon: FileText, title: 'Technical rider PDF', body: 'A file to attach to contracts, beyond the rider text in your booking details.' }
	];
</script>

<svelte:head>
	<title>{profile.name} press kit · Local Network</title>
	<meta name="description" content="Press kit for {profile.name}: {profile.tagline ?? `${kindLabel(profile.kind)} from ${placeLabel(profile)}`}" />
</svelte:head>

<div class="container-page flex flex-wrap items-center justify-between gap-3 pt-6 print:hidden">
	<a href="/projects/{project.slug}" class="menu-link inline-flex items-center gap-1.5 text-small"><ArrowLeft class="size-4" />{profile.name}</a>
	<div class="flex flex-wrap gap-2">
		<button type="button" class="btn-secondary btn-sm" onclick={copyLink}>{#if copied}<Check class="size-4" />Copied{:else}<Copy class="size-4" />Copy link{/if}</button>
		<button type="button" class="btn-primary btn-sm" onclick={() => window.print()}><Printer class="size-4" />Print or save as PDF</button>
	</div>
</div>

<article class="mx-auto flex w-full max-w-208 flex-col gap-12 px-4 py-10 sm:px-8 print:max-w-none print:gap-8 print:px-0 print:py-0">
	<header class="flex flex-col gap-5 border-b border-line pb-10 print:pb-6">
		<p class="m-0 eyebrow text-fg-subtle">Press kit · Updated {formatDate(project.updatedAt)}</p>
		<div class="flex flex-col gap-5 sm:flex-row sm:items-center">
			<Avatar src={project.avatarUrl} name={profile.name} size="xl" />
			<div class="flex min-w-0 flex-col gap-2">
				<h1 class="m-0 text-title font-semibold">{profile.name}</h1>
				{#if profile.tagline}<p class="m-0 text-lede text-fg-soft">{profile.tagline}</p>{/if}
				<p class="m-0 text-small text-fg-muted">{[kindLabel(profile.kind), placeLabel(profile), profile.pronouns].filter(Boolean).join(' · ')}</p>
			</div>
		</div>
		{#if profile.genres.length}<p class="m-0 font-mono text-label text-fg-muted">{profile.genres.join(' · ')}</p>{/if}
	</header>

	<div class="flex flex-wrap gap-x-12 gap-y-10">
		<section aria-labelledby="epk-bio" class="flex min-w-0 flex-[999_1_26rem] flex-col gap-3">
			<h2 id="epk-bio" class="m-0 text-heading font-semibold">Bio</h2>
			{#if profile.bio}
				<p class="m-0 whitespace-pre-line text-fg-soft">{profile.bio}</p>
			{:else if data.isManager}
				<p class="m-0 text-small text-fg-muted">No bio yet. <a class="link" href="/projects/{project.slug}/edit">Add one to your profile</a> and it shows here.</p>
			{:else}
				<p class="m-0 text-fg-soft">{profile.tagline ?? `${kindLabel(profile.kind)} from ${placeLabel(profile)}.`}</p>
			{/if}
			{#if profile.highlights?.length}
				<h3 class="m-0 mt-4 eyebrow text-fg-subtle">Highlights</h3>
				<ul class="m-0 flex list-none flex-col gap-2 p-0">
					{#each profile.highlights as highlight, index (index)}<li class="border-l-2 border-line-strong pl-3 text-fg-soft">{highlight}</li>{/each}
				</ul>
			{/if}
		</section>
		<aside aria-labelledby="epk-facts" class="flex min-w-0 flex-[1_1_15rem] flex-col gap-4">
			<h2 id="epk-facts" class="m-0 eyebrow text-fg-subtle">At a glance</h2>
			<ProjectFacts {profile} />
		</aside>
	</div>

	{#await data.dj}
		<div aria-busy="true" aria-label="Loading playing stats" class="flex flex-col gap-4 print:hidden">
			<div class="h-7 w-48 animate-pulse rounded-md bg-raised"></div>
			<div class="h-28 animate-pulse rounded-xl bg-surface"></div>
		</div>
	{:then accounts}
		{#each accounts as account (account.did)}
			{#if account.profile}
				<section aria-labelledby="epk-sound-{account.did}" class="flex flex-col gap-4 print:break-inside-avoid">
					<h2 id="epk-sound-{account.did}" class="m-0 text-heading font-semibold">Sound{#if accounts.length > 1 && account.handle}<span class="text-small font-normal text-fg-muted">{' '}@{account.handle}</span>{/if}</h2>
					<DjStats profile={account.profile} />
				</section>
			{/if}
			{#if account.sets.length}
				<section aria-labelledby="epk-sets-{account.did}" class="flex flex-col gap-4">
					<h2 id="epk-sets-{account.did}" class="m-0 text-heading font-semibold">Recent sets</h2>
					<DjSets sets={account.sets} limit={6} />
				</section>
			{/if}
		{:else}
			{#if data.isManager}
				<section aria-labelledby="epk-sound-empty" class="card flex flex-col gap-2 p-5 print:hidden">
					<h2 id="epk-sound-empty" class="m-0 text-emphasis font-semibold">Add how you play</h2>
					<p class="m-0 text-small text-fg-muted">Publish your playing stats and sets from <a class="link" href="/tools/locallibrary">locallibrary</a> and the press kit shows your tempo, keys, energy and recent sets.</p>
				</section>
			{/if}
		{/each}
	{/await}

	{#if profile.mixes?.length}
		<section aria-labelledby="epk-mixes" class="flex flex-col gap-4">
			<h2 id="epk-mixes" class="m-0 text-heading font-semibold">Mixes</h2>
			<ol class="m-0 flex list-none flex-col p-0">
				{#each profile.mixes as mix, index (index)}
					<li class="flex flex-wrap items-baseline justify-between gap-x-4 gap-y-1 border-t border-line py-3 last:border-b">
						<a class="font-semibold text-fg no-underline hover:underline" href={mix.url} rel="noopener noreferrer nofollow" target="_blank">{mix.title}</a>
						<span class="flex flex-wrap gap-x-4 text-small text-fg-muted">
							{#if mix.durationMinutes}<span class="font-mono text-label">{mix.durationMinutes} min</span>{/if}
							{#if mix.year}<span class="font-mono text-label">{mix.year}</span>{/if}
							{#if mix.tracklist}<a class="link print:hidden" href="/projects/{project.slug}/mixes/{index + 1}">Tracklist</a>{/if}
							<span class="hidden break-all print:inline">{mix.url}</span>
						</span>
					</li>
				{/each}
			</ol>
		</section>
	{/if}

	{#if project.avatarUrl}
		<section aria-labelledby="epk-photos" class="flex flex-col gap-4 print:break-inside-avoid">
			<h2 id="epk-photos" class="m-0 text-heading font-semibold">Photos</h2>
			<a href={project.avatarUrl} target="_blank" rel="noopener noreferrer" class="block w-fit"><img src={project.avatarUrl} alt="{profile.name}, profile photo" class="max-h-80 rounded-xl border border-line object-cover" /></a>
		</section>
	{/if}

	{#if data.isManager}
		<section aria-labelledby="epk-missing" class="flex flex-col gap-4 print:hidden">
			<div class="flex flex-col gap-1">
				<h2 id="epk-missing" class="m-0 text-heading font-semibold">Not in your press kit yet</h2>
				<p class="m-0 text-small text-fg-muted">Only you and the other managers see this. These sections appear once you can add them.</p>
			</div>
			<ul class="m-0 grid list-none gap-3 p-0 sm:grid-cols-3">
				{#each missing as item (item.title)}
					<li class="flex flex-col gap-2 rounded-xl border border-dashed border-line-strong p-4">
						<item.icon class="size-5 text-fg-subtle" />
						<p class="m-0 text-small font-semibold">{item.title}</p>
						<p class="m-0 text-label text-fg-muted">{item.body}</p>
					</li>
				{/each}
			</ul>
		</section>
	{/if}

	<section aria-labelledby="epk-booking" class="card flex flex-col gap-4 p-5 print:break-inside-avoid">
		<h2 id="epk-booking" class="m-0 text-heading font-semibold">Booking</h2>
		<BookingDetails details={data.details} signedIn={!!data.viewer} claimed={project.managers.length > 0} loginHref="/login?next=/projects/{project.slug}/epk" />
		<div class="flex flex-wrap items-center gap-x-6 gap-y-2 text-small">
			{#if data.viewer && project.managers.length && !data.isManager}<a class="btn-primary btn-sm print:hidden" href="/bookings/new?project={project.slug}">Send inquiry</a>{/if}
			<span class="text-fg-muted">Profile: <a class="link break-all" href={data.profileUrl}>{data.profileUrl}</a></span>
		</div>
		{#if profile.links?.length}
			<ul class="m-0 flex list-none flex-wrap gap-x-5 gap-y-2 p-0 text-small">
				{#each profile.links as link, index (index)}
					<li><a class="link inline-flex items-center gap-1" href={link.url} rel="noopener noreferrer nofollow" target="_blank">{link.label ?? labelFor(LINK_KINDS, link.kind)}<ExternalLink class="size-3 print:hidden" /></a><span class="hidden break-all text-fg-muted print:inline">{' '}{link.url}</span></li>
				{/each}
			</ul>
		{/if}
	</section>

	<footer class="border-t border-line pt-4 text-caption text-fg-subtle">
		Put together by Local Network from {profile.name}'s profile and records on the AT Protocol.
	</footer>
</article>
