<script lang="ts">
	import Handle from '#lib/components/Handle.svelte';
	import StatusPill from '#lib/components/StatusPill.svelte';
	import { timeAgo } from '#lib/format.ts';
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

<h1 class="text-3xl">Overview</h1>
<div class="mt-6 grid gap-4 sm:grid-cols-2 xl:grid-cols-4">
	{#each tiles as tile (tile.label)}
		<a href={tile.href} class="card card-pad hover:border-ansi-bright-cyan"><p class="muted text-sm">{tile.label}</p><p class="mt-1 text-4xl">{tile.value}</p></a>
	{/each}
</div>

{#if data.audit.pending || data.audit.failed || data.syncErrors}
	<div class="mt-6 notice">
		{#if data.audit.pending}{data.audit.pending} audit copies are still being written. {/if}
		{#if data.audit.failed}{data.audit.failed} audit copies failed permanently. {/if}
		{#if data.syncErrors}{data.syncErrors} of {data.spaces} spaces have sync errors. {/if}
		{#if data.viewer?.isAdmin}<a class="link" href="/admin/system">Check system status</a>{/if}
	</div>
{/if}

<div class="mt-8 grid gap-6 xl:grid-cols-2">
	<section class="card card-pad">
		<div class="mb-4 flex items-center justify-between"><h2 class="section-title">Oldest pending requests</h2><a class="link text-sm" href="/admin/requests">Queue</a></div>
		<ul class="divide-y-2 divide-dashed divide-ansi-blue">
			{#each data.pending as request (request.uri)}
				<li class="flex items-center justify-between gap-3 py-3">
					<div class="min-w-0">
						<a class="menu-link" href="/admin/requests/{request.rkey}">{request.kind}: {request.project?.name ?? request.record.profile?.name}</a>
						<p class="muted text-xs"><Handle account={request.requester} /> · {timeAgo(request.createdAt)}</p>
					</div>
					<StatusPill status={request.status} />
				</li>
			{:else}
				<li class="muted py-3 text-sm">Queue is empty.</li>
			{/each}
		</ul>
	</section>
	<section class="card card-pad">
		<div class="mb-4 flex items-center justify-between"><h2 class="section-title">Latest activity</h2><a class="link text-sm" href="/admin/audit">Audit log</a></div>
		<ul class="divide-y-2 divide-dashed divide-ansi-blue">
			{#each data.recent as entry (entry.id)}
				<li class="py-3 text-sm">
					<a class="menu-link" href="/admin/audit/{entry.id}">{entry.summary}</a>
					<p class="muted text-xs"><Handle account={entry.actor} /> · {entry.action} · {timeAgo(entry.createdAt)}</p>
				</li>
			{/each}
		</ul>
	</section>
</div>
