<script lang="ts">
	import { page } from '$app/state';
	import { ArrowLeft, ExternalLink, KeyRound, Trash2 } from '@lucide/svelte';
	import EpkForm from '#lib/components/epk/EpkForm.svelte';
	import FormMessage from '#lib/components/FormMessage.svelte';
	import SignInAgain from '#lib/components/SignInAgain.svelte';
	import type { PageProps } from './$types';

	let { data, form }: PageProps = $props();

	const values = $derived(form && 'values' in form && form.values ? form.values : data.values);

	const kit = $derived(`/epk/${data.epk.did}/${data.epk.rkey}`);

	let confirming = $state(false);
</script>

<svelte:head><title>Edit {data.epk.record.title} · Local Network</title></svelte:head>

<div class="flex max-w-208 flex-col gap-8">
	<a href="/dashboard/projects/{data.project.slug}/press-kits" class="menu-link inline-flex w-fit items-center gap-1.5 text-small"><ArrowLeft class="size-4" />Press kits</a>
	<div class="flex flex-wrap items-end justify-between gap-4">
		<div class="flex flex-col gap-2">
			<p class="m-0 eyebrow text-fg-subtle">{data.project.profile.name}</p>
			<h1 class="m-0 text-title font-semibold">Edit {data.epk.record.title}</h1>
		</div>
		<div class="flex flex-wrap gap-2">
			<a class="btn-secondary btn-sm" href="{page.url.pathname}/access"><KeyRound class="size-4" />Access{#if data.waiting}<span class="pill bg-link-wash text-link">{data.waiting}</span>{/if}</a>
			<a class="btn-secondary btn-sm" href={kit}><ExternalLink class="size-4" />View</a>
		</div>
	</div>
	{#if data.saved && !form}<p role="status" class="notice notice-ok m-0">Saved to your PDS. <a class="link" href={kit}>See it as others do</a></p>{/if}
	{#if form?.reauth && form.error && data.viewer}<SignInAgain did={data.viewer.did} next={page.url.pathname + page.url.search} error={form.error} />{:else}<FormMessage {form} />{/if}
	<EpkForm {values} action="?/save" photoBase="{kit}/photos" editHref="/dashboard/projects/{data.project.slug}/profile" submitLabel="Save changes">
		{#snippet actions()}
			{#if confirming}
				<span class="text-small text-fg-muted">This deletes it and everyone's access.</span>
				<button class="btn-danger btn-sm" formaction="?/delete" formnovalidate><Trash2 class="size-4" />Delete press kit</button>
				<button type="button" class="btn-ghost btn-sm" onclick={() => (confirming = false)}>Keep it</button>
			{:else}
				<button type="button" class="btn-ghost btn-sm ml-auto" onclick={() => (confirming = true)}><Trash2 class="size-4" />Delete…</button>
			{/if}
		{/snippet}
	</EpkForm>
</div>
