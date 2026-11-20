<script lang="ts">
	import { enhance } from '$app/forms';
	import { CircleCheck, FileText, LoaderCircle, RefreshCw, TriangleAlert } from '@lucide/svelte';
	import FormMessage from '#lib/components/FormMessage.svelte';
	import MigrationPlan from '#lib/components/MigrationPlan.svelte';
	import StatusPill from '#lib/components/StatusPill.svelte';
	import { pluralize, shortDid, timeAgo } from '#lib/format.ts';
	import Card from '#lib/ui/Card.svelte';
	import SectionHeader from '#lib/ui/SectionHeader.svelte';
	import type { ScriptHealth } from '#lib/server/happyview/client.ts';
	import type { PageProps } from './$types';

	let { data, form }: PageProps = $props();

	let busy = $state(false);

	let planning = $state(false);

	const access = (open: boolean) => (open ? 'public' : 'members');

	const failing = (space: (typeof data.sync)[number]) =>
		!!space.lastError || space.repos.some((repo) => repo.lastError) || space.stuck.length > 0;

	const nextTry = (at: string) => (Date.parse(at) > Date.now() ? `next try ${timeAgo(at)}` : 'due on the next sync');

	// Jetstream runs about a second behind; a minute means HappyView's index is falling behind the network.
	const LAGGING_MS = 60_000;

	const count = new Intl.NumberFormat('en');

	const unit = (name: 'millisecond' | 'second') =>
		new Intl.NumberFormat('en', { style: 'unit', unit: name, unitDisplay: 'narrow', maximumFractionDigits: 1 });

	const [millis, seconds] = [unit('millisecond'), unit('second')];

	const duration = (ms: number) => (ms < 1000 ? millis.format(ms) : seconds.format(ms / 1000));

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
			<form method="POST" action="?/planMigration" use:enhance={() => { planning = true; return async ({ update }) => { await update(); planning = false; }; }}>
				<button class="btn-secondary w-full" disabled={planning}>{#if planning}<LoaderCircle class="size-4 animate-spin" />{:else}<FileText class="size-4" />{/if}Plan the project migration</button>
			</form>
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
	{#if form && 'plan' in form && form.plan}<MigrationPlan plan={form.plan} />{/if}

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
				{#if data.happyView.stats}
					{@const { jetstream, deadLetters } = data.happyView.stats}
					<dt class="text-label text-fg-subtle">Jetstream</dt><dd class="max-sm:mb-2">{#await jetstream}{@render ghost('Loading Jetstream status')}{:then status}{#if !status}unavailable{:else if !status.connected}<span class="text-critical">disconnected</span>{:else}connected{#if status.lagMs !== null}{' · '}<span class={status.lagMs >= LAGGING_MS ? 'text-warning' : ''}>{duration(status.lagMs)} behind</span>{/if}{/if}{/await}</dd>
					<dt class="text-label text-fg-subtle">Dead letters</dt><dd class="max-sm:mb-2">{#await deadLetters}{@render ghost('Loading dead letters')}{:then letters}{#if letters === null}unavailable{:else if letters}<span class="font-mono text-warning">{count.format(letters)}</span>{:else}none{/if}{/await}</dd>
				{/if}
				<dt class="text-label text-fg-subtle">Backups</dt><dd>{#if !data.backups}off (no bucket set){:else}{#if data.backups.lastAttempt?.error}<span class="text-critical">failed {timeAgo(data.backups.lastAttempt.at)}: {data.backups.lastAttempt.error}</span>{' · '}{/if}{#if !data.backups.latestAt}<span class={data.backups.stale ? 'text-critical' : ''}>none yet</span>{:else if data.backups.stale}<span class="text-critical">last backed up {timeAgo(data.backups.latestAt)}</span>{:else}backed up {timeAgo(data.backups.latestAt)}{/if}{/if}</dd>
			</dl>
		</Card>
	</div>

	<section class="flex flex-col gap-3">
		<h2 class="section-title">Root spaces</h2>
		<div role="region" aria-label="Root spaces" tabindex="0" class="card overflow-x-auto">
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
							<li class="font-mono break-all text-fg-subtle">{shortDid(repo.did)}{repo.rev ? ` @ ${repo.rev}` : ' · never synced'}{#if repo.lastError}<span class="text-critical">{` · ${repo.lastError}`}</span>{/if}</li>
						{:else}
							<li class="text-fg-subtle">No writers yet.</li>
						{/each}
					</ul>
					{#if space.stuck.length}
						<h3 class="text-label font-medium text-fg">Stuck in projection</h3>
						<ul class="flex flex-col gap-1 text-label">
							{#each space.stuck as record (`${record.did} ${record.collection} ${record.rkey}`)}
								<li class="text-fg-subtle">
									<p class="font-mono break-all">{record.collection}/{record.rkey}</p>
									<p><span class="font-mono">{shortDid(record.did)}</span> · {pluralize(record.attempts, 'attempt')} · {nextTry(record.nextAttemptAt)}{#if record.lastError}<span class="text-critical">{` · ${record.lastError}`}</span>{/if}</p>
								</li>
							{/each}
						</ul>
					{/if}
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

	{#if data.happyView.stats}
		<!-- Last on the page, so its rows arriving don't move anything below them. -->
		<section class="flex flex-col gap-3">
			<div class="flex flex-wrap items-baseline justify-between gap-x-4 gap-y-1">
				<h2 class="section-title">HappyView scripts</h2>
				<p class="text-label text-fg-subtle">last 24 hours</p>
			</div>
			<div class="card">
				{#await data.happyView.stats.scripts}
					<div role="status" aria-label="Loading HappyView scripts">{@render scriptTable([])}</div>
				{:then metrics}
					{#if !metrics}<p class="px-3 py-2.5 text-small text-fg-muted">Script runs are unavailable.</p>
					{:else if !metrics.enabled}<p class="px-3 py-2.5 text-small text-fg-muted">Script metrics are off in HappyView.</p>
					{:else if !metrics.scripts.length}<p class="px-3 py-2.5 text-small text-fg-muted">No scripts ran.</p>
					{:else}{@render scriptTable(metrics.scripts)}{/if}
				{/await}
			</div>
		</section>
	{/if}
</div>

{#snippet ghost(label: string)}
	<span class="inline-flex h-lh w-24 items-center align-top"><span class="h-3.5 w-full animate-pulse rounded-md bg-well"></span><span class="sr-only">{label}</span></span>
{/snippet}

<!-- With no scripts, the ghost rows of a table still loading. -->
{#snippet scriptTable(scripts: ScriptHealth[])}
	<table class="table-base">
		<thead><tr><th>Trigger</th><th class="text-right">Runs</th><th class="text-right">Failed</th><th class="text-right">p95</th></tr></thead>
		<tbody>
			{#each scripts as script (script.trigger)}
				<!-- Triggers wrap after their dots and colons on a phone, not mid-word. -->
				<tr><td class="font-mono text-label wrap-anywhere">{#each script.trigger.split(/(?<=[.:])/) as part, index (index)}{part}<wbr />{/each}</td><td class="text-right tabular-nums">{count.format(script.runs)}</td><td class={['text-right tabular-nums', script.failures > 0 && 'text-critical']}>{count.format(script.failures)}</td><td class="text-right whitespace-nowrap tabular-nums">{duration(script.p95Ms)}</td></tr>
			{:else}
				{#each { length: 3 }, row (row)}<tr aria-hidden="true"><td colspan="4"><span class="flex h-lh items-center"><span class="h-3.5 w-full animate-pulse rounded-md bg-well"></span></span></td></tr>{/each}
			{/each}
		</tbody>
	</table>
{/snippet}
