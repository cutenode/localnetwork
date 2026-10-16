<script lang="ts">
	import { ArrowLeft, ShieldCheck, ShieldX } from '@lucide/svelte';
	import DiffTable from '#lib/components/DiffTable.svelte';
	import Handle from '#lib/components/Handle.svelte';
	import StatusPill from '#lib/components/StatusPill.svelte';
	import { formatDateTime } from '#lib/format.ts';
	import Card from '#lib/ui/Card.svelte';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();

	const e = $derived(data.entry);

	const deliveries = $derived([
		{ title: 'Actor copy', status: e.actorStatus, uri: e.actorUri, error: e.actorError, attempts: e.actorAttempts },
		{ title: 'Root copy', status: e.rootStatus, uri: e.rootUri, error: e.rootError, attempts: e.rootAttempts }
	]);
</script>

<svelte:head><title>Audit #{e.seq} · Staff</title></svelte:head>

<div class="flex flex-col gap-6">
	<div>
		<a href="/admin/audit" class="inline-flex items-center gap-1.5 text-small text-fg-muted no-underline hover:text-fg"><ArrowLeft class="size-4" />Audit log</a>
		<h1 class="mt-3 text-heading font-semibold sm:text-title">{e.summary}</h1>
		<p class="mt-2 text-small text-fg-muted"><span class="font-mono">#{e.seq}</span> · {formatDateTime(e.createdAt)} · <Handle account={data.actor} />{e.actorRole ? ` (${e.actorRole})` : ''} · via {e.via}</p>
	</div>

	<div class="grid grid-cols-[minmax(0,1fr)] items-start gap-6 xl:grid-cols-[minmax(0,1fr)_20rem]">
		<div class="flex min-w-0 flex-col gap-6">
			<Card as="section">
				<dl class="grid gap-4 text-small sm:grid-cols-2">
					<div><dt class="text-label text-fg-subtle">Action</dt><dd class="mt-1 font-mono text-label">{e.action}</dd></div>
					<div><dt class="text-label text-fg-subtle">Outcome</dt><dd class="mt-1"><StatusPill status={e.outcome} /></dd></div>
					{#if e.subject}<div class="sm:col-span-2"><dt class="text-label text-fg-subtle">Subject ({e.subjectKind})</dt><dd class="mono mt-1">{e.subject}</dd></div>{/if}
					{#if e.reason}<div class="sm:col-span-2"><dt class="text-label text-fg-subtle">Reason</dt><dd class="mt-1 whitespace-pre-line">{e.reason}</dd></div>{/if}
				</dl>
			</Card>
			<Card as="section">
				<h2 class="section-title mb-3">Changes</h2>
				<DiffTable changes={data.changes} empty="No field-level changes recorded." />
			</Card>
			{#if data.details}
				<Card as="section">
					<h2 class="section-title mb-3">Details</h2>
					<pre class="code-block">{JSON.stringify(data.details, null, 2)}</pre>
				</Card>
			{/if}
			<Card as="section">
				<h2 class="section-title">Copies found by sync</h2>
				<div class="mt-3 flex flex-col gap-5">
					{#each data.copies as copy (copy.uri)}
						<div class="flex flex-col gap-1.5">
							<p class="text-small font-medium">{copy.repoDid === data.entry.actorDid ? 'Actor copy' : 'Root copy'} <span class="font-normal text-fg-muted">· synced {formatDateTime(copy.syncedAt)}</span></p>
							<p class="mono text-fg-subtle">{copy.uri}</p>
							<pre class="code-block max-h-64">{JSON.stringify(copy.record, null, 2)}</pre>
						</div>
					{:else}
						<p class="text-small text-fg-muted">No copies have been synced back yet.</p>
					{/each}
				</div>
			</Card>
		</div>
		<aside class="flex flex-col gap-6">
			<Card as="section" class="flex flex-col gap-3 text-small">
				<h2 class="flex items-center gap-2 text-emphasis font-semibold">
					{#if data.hashValid}<ShieldCheck class="size-5 text-good" />{:else}<ShieldX class="size-5 text-critical" />{/if}Hash chain
				</h2>
				<p class={data.hashValid ? 'text-fg-soft' : 'text-critical'}>{data.hashValid ? "This entry's hash matches its content." : "This entry's content does not match its hash."}</p>
				<div><p class="text-label text-fg-subtle">Hash</p><p class="mono mt-1 text-fg-soft">{e.hash}</p></div>
				<div><p class="text-label text-fg-subtle">Previous</p><p class="mono mt-1 text-fg-soft">{e.prevHash || '(first entry)'}</p></div>
			</Card>
			{#each deliveries as copy (copy.title)}
				<Card as="section" class="flex flex-col gap-3 text-small">
					<div class="flex items-center justify-between gap-3"><h2 class="text-emphasis font-semibold">{copy.title}</h2><StatusPill status={copy.status} /></div>
					{#if copy.uri}<p class="mono text-fg-subtle">{copy.uri}</p>{/if}
					{#if copy.error}<p class="text-critical">{copy.error} ({copy.attempts} attempts)</p>{/if}
				</Card>
			{/each}
		</aside>
	</div>
</div>
