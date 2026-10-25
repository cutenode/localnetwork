<script lang="ts">
	import { page } from '$app/state';
	import EpkEditor from '#lib/components/epk/EpkEditor.svelte';
	import FormMessage from '#lib/components/FormMessage.svelte';
	import SignInAgain from '#lib/components/SignInAgain.svelte';
	import type { PageProps } from './$types';

	let { data, form }: PageProps = $props();

	const values = $derived(form?.values ?? data.values);
</script>

<svelte:head><title>New press kit for {data.project.profile.name} · Local Network</title></svelte:head>

<div class="flex max-w-208 flex-col gap-8">
	<div class="flex flex-col gap-2">
		<p class="m-0 eyebrow text-fg-subtle">{data.project.profile.name}</p>
		<h1 class="m-0 text-title font-semibold">New press kit</h1>
		<p class="m-0 max-w-[60ch] text-fg-muted">Make one for each kind of booking. Save a draft for the other managers to see, or publish it now.</p>
	</div>
	{#if form?.reauth && form.error && data.viewer}<SignInAgain did={data.viewer.did} next={page.url.pathname + page.url.search} error={form.error} />{:else}<FormMessage {form} />{/if}
	<EpkEditor {values} photoBase={null} editHref="/dashboard/projects/{data.project.slug}/profile" draft={null} others={[]} published={false} />
</div>
