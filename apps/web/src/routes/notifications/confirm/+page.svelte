<script lang="ts">
	import Button from '#lib/ui/Button.svelte';
	import SectionHeader from '#lib/ui/SectionHeader.svelte';
	import type { PageProps } from './$types';

	let { data, form }: PageProps = $props();

	const state = $derived(form?.result ?? data.state);
</script>

<svelte:head>
	<title>Email · Local Network</title>
	<meta name="robots" content="noindex" />
</svelte:head>

<div class="container-page flex max-w-2xl flex-col gap-6 py-10 sm:py-14">
	{#if state === 'confirmed'}
		<SectionHeader level={1} eyebrow="Email" title="Email's on" lede="New things in your inbox will come to this address too." />
		<Button variant="secondary" href="/dashboard/inbox" class="self-start">Open your inbox</Button>
	{:else if state === 'valid'}
		<SectionHeader level={1} eyebrow="Email" title="Get your inbox by email?" lede="Confirm this is your address, and new things in your Local Network inbox will come here too." />
		<form method="POST">
			<input type="hidden" name="t" value={data.token} />
			<button type="submit" class="btn-primary">Confirm</button>
		</form>
	{:else if state === 'expired'}
		<SectionHeader level={1} eyebrow="Email" title="This link has expired" lede="Links work for a day. Turn email on again from your inbox to get a new one." />
		<Button variant="secondary" href="/dashboard/inbox" class="self-start">Open your inbox</Button>
	{:else}
		<SectionHeader level={1} eyebrow="Email" title="This link doesn't work anymore" lede="It's been used, or a newer one replaced it." />
		<Button variant="secondary" href="/dashboard/inbox" class="self-start">Open your inbox</Button>
	{/if}
</div>
