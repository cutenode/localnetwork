<script lang="ts">
	import { ArrowLeft, Check, Copy, ExternalLink, Lock, Printer } from '@lucide/svelte';
	import { page } from '$app/state';
	import Avatar from '#lib/components/Avatar.svelte';
	import BookingDetails from '#lib/components/BookingDetails.svelte';
	import DjSets from '#lib/components/DjSets.svelte';
	import DjStats from '#lib/components/DjStats.svelte';
	import EpkMixes from '#lib/components/epk/EpkMixes.svelte';
	import ManageLink from '#lib/components/ManageLink.svelte';
	import PreviewBanner from '#lib/components/PreviewBanner.svelte';
	import ProjectFacts from '#lib/components/ProjectFacts.svelte';
	import { presetFor, pressKitPath } from '#lib/epk.ts';
	import { formatDate, kindLabel, placeLabel } from '#lib/format.ts';
	import { LINK_KINDS, labelFor } from '#lib/taxonomy/options.ts';
	import type { PageProps } from './$types';

	/**
	 * A project's press kits: the ones its managers publish as records, listed first, then one put
	 * together from what's on file (the profile, its mixes, the managers' playing stats and sets, and
	 * booking details for members). That one reads like a document and prints on white.
	 */
	let { data }: PageProps = $props();

	const project = $derived(data.project);

	const profile = $derived(project.profile);

	/** A mix's tracklist page, when it has one. */
	function tracklistHref(url: string) {
		const index = profile.mixes?.findIndex((mix) => mix.url === url && mix.tracklist) ?? -1;

		return index < 0 ? null : `/projects/${project.slug}/mixes/${index + 1}`;
	}

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
</script>

<svelte:head>
	<title>{profile.name} press kit · Local Network</title>
	<meta name="description" content="Press kit for {profile.name}: {profile.tagline ?? `${kindLabel(profile.kind)} from ${placeLabel(profile)}`}" />
</svelte:head>

<div class="container-page flex flex-wrap items-center justify-between gap-3 pt-6 print:hidden">
	<a href="/projects/{project.slug}" class="menu-link inline-flex items-center gap-1.5 text-small"><ArrowLeft class="size-4" />{profile.name}</a>
	<div class="flex flex-wrap gap-2">
		<ManageLink viewer={data.viewer} {project} section="press-kits" size="sm" />
		<button type="button" class="btn-secondary btn-sm" onclick={copyLink}>{#if copied}<Check class="size-4" />Copied{:else}<Copy class="size-4" />Copy link{/if}</button>
		<button type="button" class="btn-primary btn-sm" onclick={() => window.print()}><Printer class="size-4" />Print or save as PDF</button>
	</div>
</div>

<section aria-labelledby="press-kits" class="mx-auto flex w-full max-w-208 flex-col gap-4 px-4 pt-8 sm:px-8 print:hidden">
	<PreviewBanner status={project.status} />
	<div class="flex flex-col gap-1">
		<h2 id="press-kits" class="m-0 text-heading font-semibold">Press kits</h2>
		<p class="m-0 text-small text-fg-muted">One for each kind of booking.</p>
	</div>
	<!-- One placeholder per press kit the project has, each shaped like a card with a line of title, headline and access. -->
	{#snippet none()}<p class="m-0 text-small text-fg-muted">None published yet. Below is one put together from the profile.</p>{/snippet}
	{#await data.epks}
		{#if project.counts?.pressKits === 0}
			{@render none()}
		{:else}
			<div role="status" aria-busy="true" aria-label="Loading press kits" class="grid gap-3 sm:grid-cols-2">
				{#each { length: project.counts?.pressKits ?? 1 }, index (index)}
					<div aria-hidden="true" class="card flex flex-col gap-1.5 p-4">
						<span class="flex h-[1lh] items-center text-emphasis"><span class="h-4 w-1/3 animate-pulse rounded-sm bg-raised"></span></span>
						<span class="flex h-[1lh] items-center text-small"><span class="h-3.5 w-2/3 animate-pulse rounded-sm bg-raised"></span></span>
						<span class="mt-auto pt-1"><span class="flex h-[1lh] items-center text-caption"><span class="h-3 w-1/2 animate-pulse rounded-sm bg-raised"></span></span></span>
					</div>
				{/each}
			</div>
		{/if}
	{:then epks}
		{#if epks.length}
			<ul class="m-0 grid list-none gap-3 p-0 sm:grid-cols-2">
				{#each epks as epk (epk.uri)}
					{@const preset = presetFor(epk.record.access)}
					<li>
						<a href={pressKitPath(project.slug, epk.rkey)} class="card flex h-full flex-col gap-1.5 p-4 text-fg no-underline transition-colors hover:border-line-strong">
							<span class="text-emphasis font-semibold">{epk.record.title}</span>
							{#if epk.record.headline}<span class="text-small text-fg-soft">{epk.record.headline}</span>{/if}
							<span class="mt-auto inline-flex items-center gap-1.5 pt-1 text-caption text-fg-subtle"><Lock class="size-3" aria-hidden="true" />{preset?.label ?? 'Custom access'} · Updated {formatDate(epk.record.updatedAt ?? epk.record.createdAt)}</span>
						</a>
					</li>
				{/each}
			</ul>
		{:else}
			{@render none()}
		{/if}
	{/await}
</section>

<article class="mx-auto flex w-full max-w-208 flex-col gap-12 px-4 py-10 sm:px-8 print:max-w-none print:gap-8 print:px-0 print:py-0">
	<header class="flex flex-col gap-5 border-b border-line pb-10 print:pb-6">
		<p class="m-0 eyebrow text-fg-subtle">From the profile · Updated {formatDate(project.updatedAt)}</p>
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
		<div role="status" aria-busy="true" aria-label="Loading playing stats" class="flex flex-col gap-4 print:hidden">
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
		{/each}
	{/await}

	{#if profile.mixes?.length}
		<section aria-labelledby="epk-mixes" class="flex flex-col gap-4">
			<h2 id="epk-mixes" class="m-0 text-heading font-semibold">Mixes</h2>
			<EpkMixes mixes={profile.mixes} tracklistHref={(url) => tracklistHref(url)} />
		</section>
	{/if}

	{#if project.avatarUrl}
		<section aria-labelledby="epk-photos" class="flex flex-col gap-4 print:break-inside-avoid">
			<h2 id="epk-photos" class="m-0 text-heading font-semibold">Photos</h2>
			<a href={project.avatarUrl} target="_blank" rel="noopener noreferrer" class="block w-fit"><img src={project.avatarUrl} alt="{profile.name}, profile photo" class="max-h-80 rounded-xl border border-line object-cover" /></a>
		</section>
	{/if}

	<section aria-labelledby="epk-booking" class="card flex flex-col gap-4 p-5 print:break-inside-avoid">
		<h2 id="epk-booking" class="m-0 text-heading font-semibold">Booking</h2>
		<BookingDetails details={data.details} signedIn={!!data.viewer} claimed={project.managers.length > 0} loginHref="/login?next=/projects/{project.slug}/press-kits" />
		<div class="flex flex-wrap items-center gap-x-6 gap-y-2 text-small">
			{#if data.viewer && project.managers.length && !data.isManager}<a class="btn-primary btn-sm print:hidden" href="/dashboard/bookings/new?project={project.slug}">Send inquiry</a>{/if}
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
