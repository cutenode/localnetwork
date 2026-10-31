<script lang="ts">
	import { ArrowLeft } from '@lucide/svelte';
	import DmSettings from '#lib/components/DmSettings.svelte';
	import EmailSettings from '#lib/components/EmailSettings.svelte';
	import SectionHeader from '#lib/ui/SectionHeader.svelte';
	import type { PageProps } from './$types';

	let { data, form }: PageProps = $props();
</script>

<svelte:head><title>Inbox settings · Local Network</title></svelte:head>

<div class="flex max-w-3xl flex-col gap-8">
	<a href="/dashboard/inbox" class="inline-flex items-center gap-1.5 self-start text-small text-fg-muted no-underline hover:text-fg"><ArrowLeft class="size-4" aria-hidden="true" />Inbox</a>
	<SectionHeader level={1} eyebrow="Inbox" title="Where your inbox goes" lede="Everything shows up in your inbox here. You can also get it by email or as Bluesky DMs." />
	<EmailSettings available={data.email.available} channel={data.email.channel} returned={data.email.returned} topics={data.topics} form={form?.channel === 'email' ? form : null} />
	<DmSettings available={data.dm.available} channel={data.dm.channel} availability={data.dm.availability} root={data.dm.root} topics={data.topics} form={form?.channel === 'dm' ? form : null} />
</div>
