<script lang="ts">
	import { ArrowLeft, Check, Copy, Printer, Quote } from '@lucide/svelte';
	import { page } from '$app/state';
	import Avatar from '#lib/components/Avatar.svelte';
	import DjSets from '#lib/components/DjSets.svelte';
	import DjStats from '#lib/components/DjStats.svelte';
	import EpkAccessCard from '#lib/components/epk/EpkAccessCard.svelte';
	import EpkMixes from '#lib/components/epk/EpkMixes.svelte';
	import EpkPhotos from '#lib/components/epk/EpkPhotos.svelte';
	import ManageLink from '#lib/components/ManageLink.svelte';
	import PerformerFacts from '#lib/components/PerformerFacts.svelte';
	import { pressKitPath } from '#lib/epk.ts';
	import { formatDate, kindLabel, placeLabel } from '#lib/format.ts';
	import type { PageProps } from './$types';

	/**
	 * A press kit published as records: the public part for everyone, and the rest for its project's managers and
	 * the people they let in. It reads like a document and prints on white.
	 */
	let { data }: PageProps = $props();

	const epk = $derived(data.epk);

	const record = $derived(epk.record);

	const profile = $derived(data.project.profile);

	const content = $derived(data.content);

	const base = $derived(pressKitPath(data.project.slug, epk.rkey));

	const mixes = $derived([...(record.mixes ?? []), ...(content?.mixes ?? []).map((mix) => ({ ...mix, onRequest: true }))]);

	const photos = $derived([...(record.photos ?? []), ...(content?.photos ?? []).map((photo) => ({ ...photo, onRequest: true }))]);

	const isManager = $derived(data.access.kind === 'manager');

	let copied = $state(false);

	async function copyLink() {
		try {
			await navigator.clipboard.writeText(`${page.url.origin}${base}`);
			copied = true;
			setTimeout(() => (copied = false), 2000);
		} catch {
			copied = false;
		}
	}
</script>

<svelte:head>
	<title>{profile.name}: {record.title} · Local Network</title>
	<meta name="description" content="{profile.name}'s press kit: {record.headline ?? profile.tagline ?? `${kindLabel(profile.kind)} from ${placeLabel(profile)}`}" />
</svelte:head>

<div class="container-page flex flex-wrap items-center justify-between gap-3 pt-6 print:hidden">
	<a href="/projects/{data.project.slug}/press-kits" class="menu-link inline-flex items-center gap-1.5 text-small"><ArrowLeft class="size-4" />{profile.name}'s press kits</a>
	<div class="flex flex-wrap gap-2">
		<ManageLink viewer={data.viewer} project={data.project} section={isManager ? `press-kits/${epk.rkey}` : 'press-kits'} size="sm" />
		<button type="button" class="btn-secondary btn-sm" onclick={copyLink}>{#if copied}<Check class="size-4" />Copied{:else}<Copy class="size-4" />Copy link{/if}</button>
		<button type="button" class="btn-primary btn-sm" onclick={() => window.print()}><Printer class="size-4" />Print or save as PDF</button>
	</div>
</div>

<article class="mx-auto flex w-full max-w-208 flex-col gap-12 px-4 py-10 sm:px-8 print:max-w-none print:gap-8 print:px-0 print:py-0">
	<header class="flex flex-col gap-5 border-b border-line pb-10 print:pb-6">
		<p class="m-0 eyebrow text-fg-subtle">Press kit · {record.title} · Updated {formatDate(record.updatedAt ?? record.createdAt)}</p>
		<div class="flex flex-col gap-5 sm:flex-row sm:items-center">
			<Avatar src={data.project.avatarUrl} name={profile.name} size="xl" />
			<div class="flex min-w-0 flex-col gap-2">
				<h1 class="m-0 text-title font-semibold">{profile.name}</h1>
				{#if record.headline ?? profile.tagline}<p class="m-0 text-lede text-fg-soft">{record.headline ?? profile.tagline}</p>{/if}
				<p class="m-0 text-small text-fg-muted">{[kindLabel(profile.kind), placeLabel(profile), profile.pronouns].filter(Boolean).join(' · ')}</p>
			</div>
		</div>
		{#if profile.genres.length}<p class="m-0 font-mono text-label text-fg-muted">{profile.genres.join(' · ')}</p>{/if}
	</header>

	<div class="flex flex-wrap gap-x-12 gap-y-10">
		<section aria-labelledby="epk-bio" class="flex min-w-0 flex-[999_1_26rem] flex-col gap-3">
			<h2 id="epk-bio" class="m-0 text-heading font-semibold">Bio</h2>
			<p class="m-0 whitespace-pre-line text-fg-soft">{content?.bio ?? record.bio ?? profile.bio ?? profile.tagline ?? `${kindLabel(profile.kind)} from ${placeLabel(profile)}.`}</p>
			{#if profile.highlights?.length}
				<h3 class="m-0 mt-4 eyebrow text-fg-subtle">Highlights</h3>
				<ul class="m-0 flex list-none flex-col gap-2 p-0">
					{#each profile.highlights as highlight, index (index)}<li class="border-l-2 border-line-strong pl-3 text-fg-soft">{highlight}</li>{/each}
				</ul>
			{/if}
		</section>
		<aside aria-labelledby="epk-facts" class="flex min-w-0 flex-[1_1_15rem] flex-col gap-4">
			<h2 id="epk-facts" class="m-0 eyebrow text-fg-subtle">At a glance</h2>
			<PerformerFacts performer={profile} />
		</aside>
	</div>

	{#if mixes.length}
		<section aria-labelledby="epk-mixes" class="flex flex-col gap-4">
			<h2 id="epk-mixes" class="m-0 text-heading font-semibold">Mixes</h2>
			<EpkMixes {mixes} tracklistHref={(url) => (data.tracklists[url] ? `/projects/${data.project.slug}/mixes/${data.tracklists[url]}` : null)} />
		</section>
	{/if}

	{#if photos.length}
		<section aria-labelledby="epk-photos" class="flex flex-col gap-4">
			<h2 id="epk-photos" class="m-0 text-heading font-semibold">Photos</h2>
			<EpkPhotos {photos} base="{base}/photos" />
		</section>
	{/if}

	{#await data.dj then accounts}
		{#each accounts as account (account.did)}
			{#if account.profile}
				<section aria-labelledby="epk-sound" class="flex flex-col gap-4 print:break-inside-avoid">
					<h2 id="epk-sound" class="m-0 text-heading font-semibold">Sound</h2>
					<DjStats profile={account.profile} />
				</section>
			{/if}
			{#if account.sets.length}
				<section aria-labelledby="epk-sets" class="flex flex-col gap-4">
					<h2 id="epk-sets" class="m-0 text-heading font-semibold">Recent sets</h2>
					<DjSets sets={account.sets} limit={6} />
				</section>
			{/if}
		{/each}
	{/await}

	{#if content}
		{#if content.quotes?.length}
			<section aria-labelledby="epk-quotes" class="flex flex-col gap-4">
				<h2 id="epk-quotes" class="m-0 text-heading font-semibold">Press</h2>
				<ul class="m-0 flex list-none flex-col gap-5 p-0">
					{#each content.quotes as quote, index (index)}
						<li class="flex gap-3">
							<Quote class="mt-1 size-4 shrink-0 text-fg-subtle" aria-hidden="true" />
							<figure class="m-0 flex flex-col gap-1">
								<blockquote class="m-0 text-lede text-fg-soft">{quote.text}</blockquote>
								<figcaption class="text-small text-fg-muted">{#if quote.url}<a class="link" href={quote.url} rel="noopener noreferrer nofollow" target="_blank">{quote.source}</a>{:else}{quote.source}{/if}</figcaption>
							</figure>
						</li>
					{/each}
				</ul>
			</section>
		{/if}

		<section aria-labelledby="epk-booking" class="card flex flex-col gap-4 p-5 print:break-inside-avoid">
			<h2 id="epk-booking" class="m-0 text-heading font-semibold">Booking</h2>
			{#if content.contact || content.techRider}
				<dl class="m-0 grid gap-4 text-small sm:grid-cols-2">
					{#if content.contact?.email}<div><dt class="eyebrow text-fg-subtle">Email</dt><dd class="m-0 mt-1"><a class="link" href="mailto:{content.contact.email}">{content.contact.email}</a></dd></div>{/if}
					{#if content.contact?.phone}<div><dt class="eyebrow text-fg-subtle">Phone</dt><dd class="m-0 mt-1">{content.contact.phone}</dd></div>{/if}
					{#if content.contact?.note}<div class="sm:col-span-2"><dt class="eyebrow text-fg-subtle">Note</dt><dd class="m-0 mt-1 whitespace-pre-line">{content.contact.note}</dd></div>{/if}
					{#if content.techRider}<div class="sm:col-span-2"><dt class="eyebrow text-fg-subtle">Tech rider</dt><dd class="m-0 mt-1 whitespace-pre-line">{content.techRider}</dd></div>{/if}
				</dl>
			{:else}
				<p class="m-0 text-small text-fg-muted">No contact details in this press kit.{#if data.project.managers.length && !isManager}{' '}<a class="link" href="/dashboard/bookings/new?project={data.project.slug}">Send an inquiry</a> instead.{/if}</p>
			{/if}
		</section>
	{:else if data.access.kind !== 'manager' && data.access.kind !== 'granted'}
		<EpkAccessCard access={data.access} name={profile.name} askHref="/dashboard/access/{data.project.slug}/{epk.rkey}" />
	{/if}

	<footer class="border-t border-line pt-4 text-caption text-fg-subtle">
		Published on the AT Protocol: <span class="font-mono break-all">{epk.uri}</span>
	</footer>
</article>
