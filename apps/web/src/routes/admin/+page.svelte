<script lang="ts">
	import Handle from '#lib/components/Handle.svelte';
	import StatusPill from '#lib/components/StatusPill.svelte';
	import { requestTitle, timeAgo } from '#lib/format.ts';
	import Card from '#lib/ui/Card.svelte';
	import Eyebrow from '#lib/ui/Eyebrow.svelte';
	import SectionHeader from '#lib/ui/SectionHeader.svelte';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();

	const tiles = $derived([
		{ label: 'Waiting for review', value: data.requests.pending, href: '/admin/requests' },
		{ label: 'Live projects', value: data.projects.active, href: '/admin/projects' },
		{ label: 'Unclaimed', value: data.projects.unclaimed, href: '/admin/projects?unclaimed=1' },
		{ label: 'Audit entries', value: data.audit.total, href: '/admin/audit' }
	]);
</script>

<svelte:head><title>Staff · Local Network</title></svelte:head>

<div class="flex flex-col gap-8">
	<SectionHeader level={1} title="Overview" />

	<div class="grid grid-cols-2 gap-3 xl:grid-cols-4">
		{#each tiles as tile (tile.label)}
			<Card href={tile.href} padding="sm"><Eyebrow>{tile.label}</Eyebrow><p class="mt-2 font-mono text-heading text-fg">{tile.value}</p></Card>
		{/each}
	</div>

	{#if data.audit.pending || data.audit.failed || data.syncErrors}
		<div class="notice">
			{#if data.audit.pending}{data.audit.pending} audit copies are still being written. {/if}
			{#if data.audit.failed}{data.audit.failed} audit copies failed permanently. {/if}
			{#if data.syncErrors}{data.syncErrors} of {data.spaces} spaces have sync errors. {/if}
			{#if data.viewer?.isAdmin}<a class="link" href="/admin/system">Check system status</a>{/if}
		</div>
	{/if}

	<div class="grid items-start gap-6 xl:grid-cols-2">
		<Card as="section">
			<div class="flex items-center justify-between gap-4"><h2 class="section-title">Oldest pending requests</h2><a class="link text-small" href="/admin/requests">Queue</a></div>
			<ul class="mt-2 divide-y divide-line">
				{#each data.pending as request (request.uri)}
					<li class="flex items-center justify-between gap-3 py-3">
						<div class="min-w-0">
							<a class="font-medium text-fg no-underline hover:underline" href="/admin/requests/{request.rkey}">{requestTitle(request)}</a>
							<p class="text-label text-fg-subtle"><Handle account={request.requester} /> · {timeAgo(request.createdAt)}</p>
						</div>
						<StatusPill status={request.status} />
					</li>
				{:else}
					<li class="py-3 text-small text-fg-muted">Queue is empty.</li>
				{/each}
			</ul>
		</Card>
		<Card as="section">
			<div class="flex items-center justify-between gap-4"><h2 class="section-title">Latest activity</h2><a class="link text-small" href="/admin/audit">Audit log</a></div>
			<ul class="mt-2 divide-y divide-line">
				{#each data.recent as entry (entry.id)}
					<li class="py-3 text-small">
						<a class="font-medium text-fg no-underline hover:underline" href="/admin/audit/{entry.id}">{entry.summary}</a>
						<p class="text-label text-fg-subtle"><Handle account={entry.actor} /> · <span class="font-mono">{entry.action}</span> · {timeAgo(entry.createdAt)}</p>
					</li>
				{/each}
			</ul>
		</Card>
	</div>
</div>
