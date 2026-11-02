<script lang="ts">
	import { enhance } from '$app/forms';
	import { ExternalLink, ScrollText } from '@lucide/svelte';
	import FormMessage from '#lib/components/FormMessage.svelte';
	import StatusPill from '#lib/components/StatusPill.svelte';
	import { accountLabel, formatDate, profileHref, timeAgo } from '#lib/format.ts';
	import Card from '#lib/ui/Card.svelte';
	import SectionHeader from '#lib/ui/SectionHeader.svelte';
	import type { PageProps } from './$types';

	let { data, form }: PageProps = $props();

	const name = $derived(data.profile?.record.basics.name ?? accountLabel(data.account));
</script>

<svelte:head><title>{name} · Find DJs · Staff</title></svelte:head>

<div class="flex flex-col gap-6">
	<SectionHeader level={1} title={name} lede={data.listing ? `Listed in Find DJs since ${formatDate(data.listing.createdAt)}.` : 'Not listed in Find DJs.'}>
		{#snippet actions()}
			<a class="btn-secondary" href={profileHref(data.account)}><ExternalLink class="size-4" />Profile</a>
			<a class="btn-secondary" href="/admin/audit?actor={encodeURIComponent(data.account.did)}"><ScrollText class="size-4" />Their actions</a>
			{#if data.listing}
				<form method="POST" action="?/unlist" use:enhance={({ cancel }) => { if (!confirm(`Take ${name} out of Find DJs?`)) cancel(); }}>
					<button class="btn-danger w-full">Take out of Find DJs</button>
				</form>
			{/if}
		{/snippet}
	</SectionHeader>
	<p class="flex flex-wrap items-center gap-2 text-small text-fg-muted"><StatusPill status={data.listing ? 'listed' : 'unlisted'} />{accountLabel(data.account)}</p>

	<FormMessage {form} />

	<Card as="section" padding="none">
		<h2 class="m-0 border-b border-line px-5 py-4 text-emphasis font-semibold">Requests to be listed</h2>
		<ul class="m-0 list-none divide-y divide-line p-0">
			{#each data.requests as request (request.uri)}
				<li class="flex items-center justify-between gap-3 px-5 py-3.5">
					<a class="text-fg no-underline hover:underline" href="/admin/requests/{request.rkey}">Asked {timeAgo(request.createdAt)}</a>
					<StatusPill status={request.status} />
				</li>
			{:else}
				<li class="px-5 py-6 text-small text-fg-muted">They haven't asked.</li>
			{/each}
		</ul>
	</Card>
</div>
