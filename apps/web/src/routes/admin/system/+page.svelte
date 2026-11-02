<script lang="ts">
	import { enhance } from '$app/forms';
	import { CircleCheck, RefreshCw, TriangleAlert } from '@lucide/svelte';
	import FormMessage from '#lib/components/FormMessage.svelte';
	import StatusPill from '#lib/components/StatusPill.svelte';
	import { shortDid, timeAgo } from '#lib/format.ts';
	import Card from '#lib/ui/Card.svelte';
	import SectionHeader from '#lib/ui/SectionHeader.svelte';
	import type { PageProps } from './$types';

	let { data, form }: PageProps = $props();

	let busy = $state(false);

	const access = (open: boolean) => (open ? 'public' : 'members');

	const failing = (space: (typeof data.sync)[number]) => !!space.lastError || space.repos.some((repo) => repo.lastError);

	// Spaces with errors first, so a problem isn't lost among the healthy ones.
	const spaces = $derived(data.sync.toSorted((a, b) => Number(failing(b)) - Number(failing(a))));

	const failingCount = $derived(spaces.filter(failing).length);

	const problems = $derived(
		(
			[
				[!data.happyView.reachable, "HappyView isn't answering"],
				[!!data.happyView.sync?.error, "The directory didn't sync"],
				[failingCount > 0, `${failingCount} of ${spaces.length} spaces have sync errors`],
				[data.audit.failed > 0, `${data.audit.failed} audit copies failed`],
				[!!data.backups?.lastAttempt?.error, 'The last backup failed'],
				[!!data.backups?.stale, 'No backup in two days']
			] satisfies [boolean, string][]
		).flatMap(([found, problem]) => (found ? [problem] : []))
	);
</script>

<svelte:head><title>System · Staff</title></svelte:head>

<div class="flex flex-col gap-6">
	<SectionHeader level={1} title="System" lede="Bootstrap and sync creates any missing root spaces, then re-reads every space and the directory. Run it after a deploy or when sync falls behind.">
		{#snippet actions()}
			<form method="POST" action="?/retryAudit" use:enhance><button class="btn-secondary w-full">Retry audit copies</button></form>
			<form method="POST" action="?/resync" use:enhance={({ cancel }) => { if (!confirm('Bootstrap and sync every space now? It can take a minute.')) return cancel(); busy = true; return async ({ update }) => { await update(); busy = false; }; }}>
				<button class="btn-secondary w-full" disabled={busy}><RefreshCw class="size-4 {busy ? 'animate-spin' : ''}" />Bootstrap and sync</button>
			</form>
		{/snippet}
	</SectionHeader>
	<p role="status" class={['notice flex items-start gap-2', !problems.length && 'notice-ok']}>
		{#if problems.length}<TriangleAlert aria-hidden="true" class="mt-0.5 size-4 shrink-0 text-warning" />{problems.join(' · ')}{:else}<CircleCheck aria-hidden="true" class="mt-0.5 size-4 shrink-0 text-good" />Everything is running and in sync.{/if}
	</p>
	<FormMessage {form} />

	<div class="grid items-start gap-6 xl:grid-cols-2">
		<Card as="section">
			<h2 class="section-title mb-4">Root account</h2>
			<dl class="grid gap-x-6 gap-y-1 text-small sm:grid-cols-[8rem_minmax(0,1fr)] sm:gap-y-3">
				<dt class="text-label text-fg-subtle">Handle</dt><dd class="max-sm:mb-2">{data.root.handle ? `@${data.root.handle}` : 'none'}</dd>
				<dt class="text-label text-fg-subtle">DID</dt><dd class="mono max-sm:mb-2">{data.root.did}</dd>
				<dt class="text-label text-fg-subtle">PDS</dt><dd class="break-all">{data.root.service}</dd>
			</dl>
		</Card>
		<Card as="section">
			<h2 class="section-title mb-4">Deployment</h2>
			<dl class="grid gap-x-6 gap-y-1 text-small sm:grid-cols-[8rem_minmax(0,1fr)] sm:gap-y-3">
				<dt class="text-label text-fg-subtle">Public URL</dt><dd class="break-all max-sm:mb-2">{data.config.publicUrl}</dd>
				<dt class="text-label text-fg-subtle">Syncer</dt><dd class="mono max-sm:mb-2">{data.config.serviceDid}</dd>
				<dt class="text-label text-fg-subtle">Write notifications</dt><dd class="max-sm:mb-2">{data.config.syncNotify ? 'registered' : 'off (polling only)'} · poll every {Math.round(data.config.pollMs / 1000)}s</dd>
				<dt class="text-label text-fg-subtle">Geocoder</dt><dd class="max-sm:mb-2">{data.config.geocoder}</dd>
				<dt class="text-label text-fg-subtle">HappyView</dt><dd class="break-all max-sm:mb-2">{data.happyView.url} · {#if data.happyView.reachable}<span class="text-good">answering</span>{:else}<span class="text-critical">not answering</span>{/if}</dd>
				<dt class="text-label text-fg-subtle">Directory scripts</dt><dd class="max-sm:mb-2">{#if !data.happyView.sync}managed outside this app (no API key){:else if data.happyView.sync.error}<span class="text-critical">sync failed {timeAgo(data.happyView.sync.at)}: {data.happyView.sync.error}</span>{:else}synced {timeAgo(data.happyView.sync.at)}{/if}</dd>
				<dt class="text-label text-fg-subtle">Backups</dt><dd>{#if !data.backups}off (no bucket set){:else}{#if data.backups.lastAttempt?.error}<span class="text-critical">failed {timeAgo(data.backups.lastAttempt.at)}: {data.backups.lastAttempt.error}</span>{' · '}{/if}{#if !data.backups.latestAt}<span class={data.backups.stale ? 'text-critical' : ''}>none yet</span>{:else if data.backups.stale}<span class="text-critical">last backed up {timeAgo(data.backups.latestAt)}</span>{:else}backed up {timeAgo(data.backups.latestAt)}{/if}{/if}</dd>
			</dl>
		</Card>
	</div>

	<section class="flex flex-col gap-3">
		<h2 class="section-title">Root spaces</h2>
		<div class="card overflow-x-auto">
			<table class="table-base min-w-200">
				<thead><tr><th>Space</th><th>Read</th><th>Write</th><th>URI</th></tr></thead>
				<tbody>
					{#each data.spaces as space (space.uri)}
						<tr><td class="font-medium capitalize">{space.name}</td><td class="font-mono text-label">{access(space.policy.publicRead)}</td><td class="font-mono text-label">{access(space.policy.publicWrite)}</td><td class="mono text-fg-subtle">{space.uri}</td></tr>
					{/each}
				</tbody>
			</table>
		</div>
	</section>

	<section class="flex flex-col gap-3">
		<h2 class="section-title">Sync status</h2>
		<div class="grid gap-3 lg:grid-cols-2">
			{#each spaces as space (space.space)}
				<Card padding="sm" class="flex flex-col gap-2">
					<div class="flex flex-wrap items-baseline justify-between gap-x-3 gap-y-1">
						<p class="flex items-baseline gap-2 font-mono text-label text-fg">{space.type.split('.').at(-1)} / {space.skey}{#if failing(space)}<StatusPill status="failed" />{/if}</p>
						<p class="text-label text-fg-subtle">{space.lastSyncedAt ? `synced ${timeAgo(space.lastSyncedAt)}` : 'never synced'}{space.registrationExpiresAt ? ` · notify until ${timeAgo(space.registrationExpiresAt)}` : ''}</p>
					</div>
					{#if space.lastError}<p class="text-small text-critical">{space.lastError}</p>{/if}
					<ul class="flex flex-col gap-1 text-label">
						{#each space.repos as repo (repo.did)}
							<li class="font-mono break-all text-fg-subtle">{shortDid(repo.did)} @ {repo.rev}{#if repo.lastError}<span class="text-critical"> · {repo.lastError}</span>{/if}</li>
						{:else}
							<li class="text-fg-subtle">No writers yet.</li>
						{/each}
					</ul>
				</Card>
			{/each}
		</div>
	</section>

	<Card as="section">
		<div class="flex flex-wrap items-baseline justify-between gap-x-4 gap-y-1">
			<h2 class="section-title">Audit delivery</h2>
			<p class="text-label text-fg-subtle"><span class="font-mono">{data.audit.pending}</span> pending · <span class="font-mono">{data.audit.failed}</span> failed</p>
		</div>
		<ul class="mt-1 divide-y divide-line text-small">
			{#each data.failed as entry (entry.id)}
				<li class="flex flex-wrap items-center gap-2 py-2.5">
					<a class="text-fg no-underline hover:underline" href="/admin/audit/{entry.id}"><span class="font-mono text-fg-subtle">#{entry.seq}</span> {entry.summary}</a>
					<StatusPill status={entry.actorStatus} /><StatusPill status={entry.rootStatus} />
					{#if entry.actorError ?? entry.rootError}<span class="basis-full text-label text-critical">{entry.actorError ?? entry.rootError}</span>{/if}
				</li>
			{:else}
				<li class="py-2.5 text-fg-muted">Every audit copy has been delivered.</li>
			{/each}
		</ul>
	</Card>
</div>
