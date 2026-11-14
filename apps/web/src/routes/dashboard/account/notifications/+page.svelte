<script lang="ts">
	import DmSettings from '#lib/components/DmSettings.svelte';
	import EmailSettings from '#lib/components/EmailSettings.svelte';
	import type { PageProps } from './$types';

	let { data, form }: PageProps = $props();
</script>

<svelte:head><title>Notifications · Local Network</title></svelte:head>

<div class="flex max-w-3xl flex-col gap-8">
	<header class="flex flex-col gap-1">
		<h1 class="m-0 text-heading font-semibold">Notifications</h1>
		<p class="m-0 text-body text-fg-muted">Everything shows up in your inbox here. You can also get it by email or as Bluesky DMs.</p>
	</header>
	<EmailSettings available={data.email.available} channel={data.email.channel} returned={data.email.returned} topics={data.topics} form={form?.channel === 'email' ? form : null} />
	<DmSettings available={data.dm.available} channel={data.dm.channel} availability={data.dm.availability} root={data.dm.root} topics={data.topics} form={form?.channel === 'dm' ? form : null} />
</div>
