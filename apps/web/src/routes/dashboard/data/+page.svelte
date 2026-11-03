<script lang="ts">
	import HeldRecordList from '#lib/components/HeldRecordList.svelte';
	import SectionHeader from '#lib/ui/SectionHeader.svelte';
	import type { PageProps } from './$types';

	let { data, form }: PageProps = $props();
</script>

<svelte:head><title>Your data · Local Network</title></svelte:head>

<div class="flex max-w-4xl flex-col gap-8">
	<div class="flex flex-col gap-3">
		<SectionHeader level={1} eyebrow="Privacy" title="Your data" lede="What Local Network wrote for you when your account's server couldn't. You can delete any of it." />
		<p class="m-0 text-small text-fg-muted">Anything you wrote with your own account is already in your own repo. Audit log entries stay, since each is chained to the one before, and so do booking status changes, which set the thread's status for both sides.</p>
	</div>
	<div class="flex flex-col gap-6"><HeldRecordList records={data.records} {form} /></div>
</div>
