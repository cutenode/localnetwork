<script lang="ts">
	import { ChevronDown } from '@lucide/svelte';
	import { AUDIT_COPY_LABEL, auditSentence } from '#lib/audit-sentences.ts';
	import { formatDateTime, timeAgo } from '#lib/format.ts';
	import type { AuditEntryRow } from '#lib/server/db/schema.ts';

	type Entry = Pick<AuditEntryRow, 'id' | 'action' | 'summary' | 'outcome' | 'createdAt' | 'actorStatus' | 'rootStatus'>;

	/**
	 * What someone did, newest first, each a sentence with when it happened. The rest of an entry sits behind Details,
	 * which also carries the exact time, since a touch screen has no hover. `loading` holds the room of a page of them.
	 */
	let { entries = [], loading = false }: { entries?: Entry[]; loading?: boolean } = $props();
</script>

<ol class="card m-0 list-none divide-y divide-line overflow-hidden p-0" aria-busy={loading || undefined}>
	{#if loading}
		{#each { length: 6 }, row (row)}
			<li class="flex items-center gap-3 px-4 py-3 sm:gap-4 sm:px-5 sm:py-3.5">
				<span class="flex flex-1 flex-col gap-0.5">
					<span class="flex min-h-lh items-center text-small"><span class="h-3.5 w-3/5 rounded-md bg-well motion-safe:animate-pulse"></span></span>
					<span class="flex min-h-lh items-center text-label"><span class="h-3 w-24 rounded-md bg-well motion-safe:animate-pulse"></span></span>
				</span>
			</li>
		{/each}
	{/if}
	{#each entries as entry (entry.id)}
		<li>
			<details class="group">
				<summary class="flex cursor-pointer list-none items-center gap-3 px-4 py-3 transition-colors group-open:bg-raised hover:bg-raised sm:gap-4 sm:px-5 sm:py-3.5 [&::-webkit-details-marker]:hidden">
					<span class="flex min-w-0 flex-1 flex-col gap-0.5">
						<span class="text-small font-medium break-words text-fg">{auditSentence(entry)}</span>
						<time class="text-label text-fg-muted" datetime={entry.createdAt} title={formatDateTime(entry.createdAt)}>{timeAgo(entry.createdAt)}</time>
					</span>
					<span class="flex shrink-0 items-center gap-1 text-label text-fg-muted">Details<ChevronDown class="size-3.5 transition-transform motion-reduce:transition-none group-open:rotate-180" aria-hidden="true" /></span>
				</summary>
				<dl class="m-0 grid grid-cols-[auto_1fr] gap-x-6 gap-y-1.5 bg-raised px-4 pt-1 pb-4 text-label sm:px-5">
					<dt class="text-fg-subtle">When</dt>
					<dd class="m-0 text-fg-soft">{formatDateTime(entry.createdAt)}</dd>
					<dt class="text-fg-subtle">Action</dt>
					<dd class="m-0 font-mono break-all text-fg-soft">{entry.action}</dd>
					<dt class="text-fg-subtle">Your copy</dt>
					<dd class="m-0 text-fg-soft">{AUDIT_COPY_LABEL[entry.actorStatus]}</dd>
					<dt class="text-fg-subtle">Site copy</dt>
					<dd class="m-0 text-fg-soft">{AUDIT_COPY_LABEL[entry.rootStatus]}</dd>
				</dl>
			</details>
		</li>
	{/each}
</ol>
