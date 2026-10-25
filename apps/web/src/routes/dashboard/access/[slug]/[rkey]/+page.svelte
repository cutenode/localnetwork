<script lang="ts">
	import { ArrowLeft } from '@lucide/svelte';
	import Avatar from '#lib/components/Avatar.svelte';
	import EpkAccessCard from '#lib/components/epk/EpkAccessCard.svelte';
	import { pressKitPath } from '#lib/epk.ts';
	import type { PageProps } from './$types';

	let { data, form }: PageProps = $props();

	const record = $derived(data.epk.record);

	const name = $derived(data.project.profile.name);

	const kit = $derived(pressKitPath(data.project.slug, data.epk.rkey));
</script>

<svelte:head><title>Ask for {record.title} · Local Network</title></svelte:head>

<div class="flex max-w-208 flex-col gap-8">
	<a href={kit} class="menu-link inline-flex w-fit items-center gap-1.5 text-small"><ArrowLeft class="size-4" />Back to the press kit</a>
	<header class="flex items-center gap-4">
		<Avatar src={data.project.avatarUrl} name={name} size="lg" />
		<div class="flex min-w-0 flex-col gap-1">
			<p class="m-0 eyebrow text-fg-subtle">Press kit · {name}</p>
			<h1 class="m-0 text-title font-semibold break-words">{record.title}</h1>
			{#if record.headline}<p class="m-0 text-small text-fg-soft">{record.headline}</p>{/if}
		</div>
	</header>
	{#if data.access.kind === 'granted'}
		<section aria-labelledby="epk-access" class="card flex flex-col gap-3 p-5">
			<h2 id="epk-access" class="m-0 text-emphasis font-semibold">You can see all of it</h2>
			<p class="m-0"><a class="btn-primary btn-sm" href={kit}>Open the press kit</a></p>
		</section>
	{:else}
		<EpkAccessCard access={data.access} {name} error={form?.error} />
	{/if}
</div>
