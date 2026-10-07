<script lang="ts">
	import { enhance } from '$app/forms';
	import { RefreshCw } from '@lucide/svelte';
	import FormMessage from '#lib/components/FormMessage.svelte';
	import StatusPill from '#lib/components/StatusPill.svelte';
	import { shortDid, timeAgo } from '#lib/format.ts';
	import type { PageProps } from './$types';

	let { data, form }: PageProps = $props();

	let busy = $state(false);
</script>

<svelte:head><title>System · Staff</title></svelte:head>

<div class="flex flex-wrap items-center justify-between gap-4">
	<h1 class="text-3xl">System</h1>
	<div class="flex gap-2">
		<form method="POST" action="?/retryAudit" use:enhance><button class="btn-secondary">Retry audit copies</button></form>
		<form method="POST" action="?/resync" use:enhance={() => { busy = true; return async ({ update }) => { await update(); busy = false; }; }}>
			<button class="btn-primary" disabled={busy}><RefreshCw class="size-4 {busy ? 'animate-spin' : ''}" />Bootstrap and sync</button>
		</form>
	</div>
</div>
<div class="mt-4"><FormMessage {form} /></div>

<div class="mt-6 grid gap-6 xl:grid-cols-2">
	<section class="card card-pad space-y-2 text-sm">
		<h2 class="section-title">Root account</h2>
		<p><span class="muted">Handle:</span> {data.root.handle ? `@${data.root.handle}` : 'none'}</p>
		<p class="mono">{data.root.did}</p>
		<p><span class="muted">PDS:</span> {data.root.service}</p>
	</section>
	<section class="card card-pad space-y-2 text-sm">
		<h2 class="section-title">Deployment</h2>
		<p><span class="muted">Public URL:</span> {data.config.publicUrl}</p>
		<p><span class="muted">Syncer:</span> <span class="mono">{data.config.serviceDid}</span></p>
		<p><span class="muted">Write notifications:</span> {data.config.syncNotify ? 'registered' : 'off (polling only)'} · poll every {Math.round(data.config.pollMs / 1000)}s</p>
		<p><span class="muted">Geocoder:</span> {data.config.geocoder}</p>
		<p><span class="muted">HappyView:</span> {data.happyView.url} · {#if data.happyView.reachable}answering{:else}<span class="text-ansi-bright-red">not answering</span>{/if}</p>
		<p><span class="muted">Directory scripts:</span> {#if !data.happyView.sync}managed outside this app (no API key){:else if data.happyView.sync.error}<span class="text-ansi-bright-red">sync failed {timeAgo(data.happyView.sync.at)}: {data.happyView.sync.error}</span>{:else}synced {timeAgo(data.happyView.sync.at)}{/if}</p>
	</section>
</div>

<section class="card card-pad mt-6">
	<h2 class="section-title mb-3">Root spaces</h2>
	<div class="overflow-x-auto">
		<table class="table-base">
			<thead><tr><th>Space</th><th>Read</th><th>Write</th><th>URI</th></tr></thead>
			<tbody>
				{#each data.spaces as space (space.uri)}
					<tr><td class="text-ansi-white capitalize">{space.name}</td><td>{space.policy.readPolicy.$type.split('#')[1]}</td><td>{space.policy.writePolicy.$type.split('#')[1]}</td><td class="mono muted">{space.uri}</td></tr>
				{/each}
			</tbody>
		</table>
	</div>
</section>

<section class="card card-pad mt-6">
	<h2 class="section-title mb-3">Sync status</h2>
	<div class="space-y-4">
		{#each data.sync as space (space.space)}
			<div class="border-2 border-ansi-blue p-4">
				<div class="flex flex-wrap items-center justify-between gap-2">
					<p class="text-ansi-white">{space.type.split('.').at(-1)} / {space.skey}</p>
					<p class="muted text-xs">{space.lastSyncedAt ? `synced ${timeAgo(space.lastSyncedAt)}` : 'never synced'}{space.registrationExpiresAt ? ` · notify until ${timeAgo(space.registrationExpiresAt)}` : ''}</p>
				</div>
				{#if space.lastError}<p class="mt-1 text-sm text-ansi-bright-red">{space.lastError}</p>{/if}
				<ul class="mt-2 grid gap-1 text-xs sm:grid-cols-2">
					{#each space.repos as repo (repo.did)}
						<li class="mono muted">{shortDid(repo.did)} @ {repo.rev}{repo.lastError ? ` · ${repo.lastError}` : ''}</li>
					{:else}
						<li class="muted">No writers yet.</li>
					{/each}
				</ul>
			</div>
		{/each}
	</div>
</section>

<section class="card card-pad mt-6">
	<h2 class="section-title mb-3">Audit delivery ({data.audit.pending} pending, {data.audit.failed} failed)</h2>
	<ul class="space-y-2 text-sm">
		{#each data.failed as entry (entry.id)}
			<li class="flex flex-wrap items-center gap-2"><a class="hover:underline" href="/admin/audit/{entry.id}">#{entry.seq} {entry.summary}</a><StatusPill status={entry.actorStatus} /><StatusPill status={entry.rootStatus} /><span class="muted text-xs">{entry.actorError ?? entry.rootError ?? ''}</span></li>
		{:else}
			<li class="muted">Every audit copy has been delivered.</li>
		{/each}
	</ul>
</section>
