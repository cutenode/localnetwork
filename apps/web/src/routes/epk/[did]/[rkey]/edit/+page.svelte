<script lang="ts">
	import { ArrowLeft, Trash2 } from '@lucide/svelte';
	import EpkForm from '#lib/components/epk/EpkForm.svelte';
	import FormMessage from '#lib/components/FormMessage.svelte';
	import type { PageProps } from './$types';

	let { data, form }: PageProps = $props();

	const values = $derived(form && 'values' in form && form.values ? form.values : data.values);

	const base = $derived(`/epk/${data.epk.did}/${data.epk.rkey}`);

	let confirming = $state(false);
</script>

<svelte:head><title>Edit {data.epk.record.title} · Local Network</title></svelte:head>

<div class="container-page flex max-w-208 flex-col gap-8 py-10 sm:py-14">
	<a href={base} class="menu-link inline-flex w-fit items-center gap-1.5 text-small"><ArrowLeft class="size-4" />Back to the press kit</a>
	<div class="flex flex-col gap-2">
		<p class="m-0 eyebrow text-fg-subtle">{data.project.profile.name}</p>
		<h1 class="m-0 text-title font-semibold">Edit {data.epk.record.title}</h1>
	</div>
	<FormMessage {form} />
	<EpkForm {values} action="?/save" photoBase="{base}/photos" editHref="/projects/{data.project.slug}/edit" submitLabel="Save changes">
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
