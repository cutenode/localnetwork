<script lang="ts">
	import { ShieldCheck, ShieldX } from '@lucide/svelte';
	import DiffTable from '#lib/components/DiffTable.svelte';
	import Handle from '#lib/components/Handle.svelte';
	import StatusPill from '#lib/components/StatusPill.svelte';
	import { formatDateTime } from '#lib/format.ts';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();

	const e = $derived(data.entry);
</script>

<svelte:head><title>Audit #{e.seq} · Staff</title></svelte:head>

<a href="/admin/audit" class="link text-sm">← Audit log</a>
<h1 class="mt-3 text-2xl">{e.summary}</h1>
<p class="muted mt-1 text-sm">#{e.seq} · {formatDateTime(e.createdAt)} · <Handle account={data.actor} />{e.actorRole ? ` (${e.actorRole})` : ''} · via {e.via}</p>

<div class="mt-6 grid gap-6 xl:grid-cols-[1fr_22rem]">
	<div class="space-y-6">
		<section class="card card-pad grid gap-3 text-sm sm:grid-cols-2">
			<div><p class="muted">Action</p><p class="mono">{e.action}</p></div>
			<div><p class="muted">Outcome</p><StatusPill status={e.outcome} /></div>
			{#if e.subject}<div class="sm:col-span-2"><p class="muted">Subject ({e.subjectKind})</p><p class="mono">{e.subject}</p></div>{/if}
			{#if e.reason}<div class="sm:col-span-2"><p class="muted">Reason</p><p class="whitespace-pre-line">{e.reason}</p></div>{/if}
		</section>
		<section class="card card-pad">
			<h2 class="section-title mb-3">Changes</h2>
			<DiffTable changes={data.changes} empty="No field-level changes recorded." />
		</section>
		{#if data.details}
			<section class="card card-pad">
				<h2 class="section-title mb-3">Details</h2>
				<pre class="code-block">{JSON.stringify(data.details, null, 2)}</pre>
			</section>
		{/if}
		<section class="card card-pad space-y-4">
			<h2 class="section-title">Copies found by sync</h2>
			{#each data.copies as copy (copy.uri)}
				<div class="space-y-1">
					<p class="text-sm">{copy.repoDid === data.entry.actorDid ? 'Actor copy' : 'Root copy'} · synced {formatDateTime(copy.syncedAt)}</p>
					<p class="mono muted">{copy.uri}</p>
					<pre class="code-block max-h-64">{JSON.stringify(copy.record, null, 2)}</pre>
				</div>
			{:else}
				<p class="muted text-sm">No copies have been synced back yet.</p>
			{/each}
		</section>
	</div>
	<aside class="space-y-6">
		<section class="card card-pad space-y-3 text-sm">
			<h2 class="section-title flex items-center gap-2">{#if data.hashValid}<ShieldCheck class="size-5 text-ansi-bright-green" />{:else}<ShieldX class="size-5 text-ansi-bright-red" />{/if}Hash chain</h2>
			<p>{data.hashValid ? "This entry's hash matches its content." : "This entry's content does not match its hash."}</p>
			<div><p class="muted">Hash</p><p class="mono">{e.hash}</p></div>
			<div><p class="muted">Previous</p><p class="mono">{e.prevHash || '(first entry)'}</p></div>
		</section>
		<section class="card card-pad space-y-3 text-sm">
			<h2 class="section-title">Actor copy</h2>
			<StatusPill status={e.actorStatus} />
			{#if e.actorUri}<p class="mono muted">{e.actorUri}</p>{/if}
			{#if e.actorError}<p class="text-ansi-bright-red">{e.actorError} ({e.actorAttempts} attempts)</p>{/if}
		</section>
		<section class="card card-pad space-y-3 text-sm">
			<h2 class="section-title">Root copy</h2>
			<StatusPill status={e.rootStatus} />
			{#if e.rootUri}<p class="mono muted">{e.rootUri}</p>{/if}
			{#if e.rootError}<p class="text-ansi-bright-red">{e.rootError} ({e.rootAttempts} attempts)</p>{/if}
		</section>
	</aside>
</div>
