<script lang="ts">
	import Button from '#lib/ui/Button.svelte';
	import SectionHeader from '#lib/ui/SectionHeader.svelte';
	import type { PageProps } from './$types';

	let { data, form }: PageProps = $props();
</script>

<svelte:head>
	<title>Email · Local Network</title>
	<meta name="robots" content="noindex" />
</svelte:head>

<div class="container-page flex max-w-2xl flex-col gap-6 py-10 sm:py-14">
	{#if form?.done}
		<SectionHeader level={1} eyebrow="Email" title="Email's off" lede="You won't get Local Network email anymore. Everything still shows up in your inbox on the site." />
		<Button variant="secondary" href="/dashboard/inbox" class="self-start">Open your inbox</Button>
	{:else if data.valid}
		<SectionHeader level={1} eyebrow="Email" title="Turn off Local Network email?" lede="You'll stop getting updates by email. Your inbox on the site keeps everything." />
		<form method="POST">
			<input type="hidden" name="t" value={data.token} />
			<button type="submit" class="btn-primary">Turn email off</button>
		</form>
	{:else}
		<SectionHeader level={1} eyebrow="Email" title="Email's already off" lede="This link has been used, or email was turned off another way." />
		<Button variant="secondary" href="/dashboard/inbox" class="self-start">Open your inbox</Button>
	{/if}
</div>
