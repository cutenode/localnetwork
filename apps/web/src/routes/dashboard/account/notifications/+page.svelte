<script lang="ts">
	import ChannelTopics from '#lib/components/ChannelTopics.svelte';
	import DmSettings from '#lib/components/DmSettings.svelte';
	import EmailSettings from '#lib/components/EmailSettings.svelte';
	import type { PageProps } from './$types';

	let { data, form }: PageProps = $props();

	// Only channels the site can send by get a column; the cards below say when one isn't set up.
	const channels = $derived([
		...(data.email.available ? [{ channel: 'email' as const, view: data.email.channel }] : []),
		...(data.dm.available ? [{ channel: 'dm' as const, view: data.dm.channel }] : [])
	]);
</script>

<svelte:head><title>Notifications · Local Network</title></svelte:head>

<div class="flex max-w-3xl flex-col gap-8 sm:gap-10">
	<header class="flex flex-col gap-1">
		<h1 class="m-0 text-heading font-semibold">Notifications</h1>
		<p class="m-0 text-body text-fg-muted">Your inbox here gets everything. Pick what else goes by email or Bluesky DM.</p>
	</header>
	<ChannelTopics topics={data.topics} {channels} />
	<div class="flex flex-col gap-3">
		<EmailSettings available={data.email.available} channel={data.email.channel} returned={data.email.returned} form={form?.channel === 'email' ? form : null} />
		<DmSettings available={data.dm.available} channel={data.dm.channel} availability={data.dm.availability} root={data.dm.root} form={form?.channel === 'dm' ? form : null} />
	</div>
</div>
