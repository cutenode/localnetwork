<script lang="ts">
	import { ExternalLink } from '@lucide/svelte';
	import Avatar from '#lib/components/Avatar.svelte';
	import DjSets from '#lib/components/DjSets.svelte';
	import { setName, setPlace } from '#lib/dj.ts';
	import { formatShowDate, profileHref } from '#lib/format.ts';
	import { webHref } from '#lib/web-url.ts';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();
</script>

<div class="border-b border-line">
	<div class="container-page flex flex-col gap-5 py-10 sm:py-14">
		<a href={profileHref(data.account)} class="flex w-fit items-center gap-2.5 font-semibold text-fg no-underline hover:underline">
			<Avatar src={data.avatarUrl} name={data.name} size="sm" />{data.name}
		</a>
		<div class="flex flex-col gap-2">
			<h1 class="m-0 text-title font-semibold">{setName(data.set)}</h1>
			<!-- Plain text, as set lines are elsewhere: an icon only this page and one other use would join what every page loads (vite.config.ts). -->
			<p class="m-0 text-small text-fg-muted">
				{[formatShowDate(data.set.date), setPlace(data.set)].filter(Boolean).join(' · ')}
				{#await data.event}
					· <span aria-hidden="true" class="inline-block h-3.5 w-40 max-w-full animate-pulse rounded-sm bg-raised align-middle"></span><span class="sr-only">Loading the event</span>
				{:then event}
					{#if event}· Part of {event}{/if}
				{/await}
			</p>
		</div>
	</div>
</div>

<div class="container-page py-10">
	<div class="flex max-w-3xl flex-col gap-10">
	<DjSets sets={[data.set]} />

	{#await data.mixes then mixes}
		{#if mixes?.length}
			<section aria-labelledby="mixes" class="flex flex-col gap-3">
				<h2 id="mixes" class="m-0 text-heading font-semibold">Shared as a mix</h2>
				<ul class="m-0 flex list-none flex-col gap-1.5 p-0 text-small text-fg-muted">
					{#each mixes as mix, index (index)}
						<li>
							{#if mix.report}
								<a class="link" href="/projects/{mix.project.slug}/mixes/{mix.report}">{mix.title}</a>
							{:else}
								<a class="link inline-flex items-center gap-1" href={webHref(mix.url)} rel="noopener noreferrer nofollow" target="_blank">{mix.title}<ExternalLink class="size-3" /></a>
							{/if}
							on <a class="link" href="/projects/{mix.project.slug}">{mix.project.name}</a>
						</li>
					{/each}
				</ul>
			</section>
			{/if}
		{/await}
	</div>
</div>
