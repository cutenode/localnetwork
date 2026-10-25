<script lang="ts">
	import { page } from '$app/state';
	import { ArrowLeft, ExternalLink, KeyRound } from '@lucide/svelte';
	import EpkEditor from '#lib/components/epk/EpkEditor.svelte';
	import FormMessage from '#lib/components/FormMessage.svelte';
	import Handle from '#lib/components/Handle.svelte';
	import SignInAgain from '#lib/components/SignInAgain.svelte';
	import { pressKitPath } from '#lib/epk.ts';
	import type { PageProps } from './$types';

	let { data, form }: PageProps = $props();

	const values = $derived(form && 'values' in form && form.values ? form.values : data.values);

	const conflicts = $derived(form && 'conflicts' in form ? form.conflicts : undefined);

	const base = $derived(`/dashboard/projects/${data.project.slug}/press-kits/${page.params.rkey}`);
</script>

<svelte:head><title>{data.title} · Local Network</title></svelte:head>

<div class="flex max-w-208 flex-col gap-8">
	<a href="/dashboard/projects/{data.project.slug}/press-kits" class="menu-link inline-flex w-fit items-center gap-1.5 text-small"><ArrowLeft class="size-4" />Press kits</a>
	<div class="flex flex-wrap items-end justify-between gap-4">
		<div class="flex min-w-0 flex-col gap-2">
			<p class="m-0 eyebrow text-fg-subtle">{data.project.profile.name}</p>
			<h1 class="m-0 text-title font-semibold break-words">{data.title}</h1>
		</div>
		{#if data.epk && !data.legacyMaker}
			<div class="flex flex-wrap gap-2">
				<a class="btn-secondary btn-sm" href="{base}/access"><KeyRound class="size-4" />Access{#if data.waiting}<span class="pill bg-link-wash text-link">{data.waiting}</span>{/if}</a>
				<a class="btn-secondary btn-sm" href={pressKitPath(data.project.slug, data.epk.rkey)}><ExternalLink class="size-4" />View</a>
			</div>
		{/if}
	</div>
	{#if data.legacyMaker}
		<p class="notice m-0">This press kit moves to the project the next time <Handle account={data.legacyMaker} /> opens the dashboard. Until then, only they can change it.</p>
	{:else}
		{#if data.notice && !form}<p role="status" class="notice notice-ok m-0">{data.notice}</p>{/if}
		{#if form && 'reauth' in form && form.reauth && form.error && data.viewer}<SignInAgain did={data.viewer.did} next={page.url.pathname} error={form.error} />{:else}<FormMessage form={form && 'error' in form ? form : null} />{/if}
		<EpkEditor {values} photoBase="{base}/photos" editHref="/dashboard/projects/{data.project.slug}/profile" draft={data.draft} others={data.others} published={!!data.epk} {conflicts} />
	{/if}
</div>
