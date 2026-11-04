<script module lang="ts">
	import type { HeldRecordKind } from '#lib/types.ts';

	/** Each kind's heading and what deleting one does, in the order they show. */
	const GROUPS: { kind: HeldRecordKind; label: string; hint: string }[] = [
		{ kind: 'request', label: 'Submissions', hint: 'Deleting a pending one withdraws it. A decided one keeps its decision.' },
		{ kind: 'booking', label: 'Booking messages', hint: 'Inquiries and messages. Deleting one takes it out of the thread for everyone.' },
		{ kind: 'epkRequest', label: 'Press kit requests', hint: 'Deleting a pending one withdraws it. An answer stays.' },
		{ kind: 'epkDraft', label: 'Press kit drafts', hint: "Changes you haven't published yet." }
	];
</script>

<script lang="ts">
	import { enhance } from '$app/forms';
	import { formatDateTime, pluralize } from '#lib/format.ts';
	import type { HeldRecordView } from '#lib/types.ts';
	import Button from '#lib/ui/Button.svelte';
	import Card from '#lib/ui/Card.svelte';
	import EmptyState from './EmptyState.svelte';
	import FormMessage from './FormMessage.svelte';

	/**
	 * The records Local Network wrote for the viewer, by kind, each with a delete, and a delete for each kind. Keeps
	 * showing the last list while a new one loads, so deleting doesn't flash the placeholder.
	 */
	let { records, form }: { records: Promise<HeldRecordView[]>; form?: { error?: string; success?: string } | null } = $props();

	let loaded = $state<{ list: HeldRecordView[] } | 'failed' | null>(null);

	$effect(() => {
		let current = true;

		records.then(
			(list) => current && (loaded = { list }),
			() => current && (loaded = 'failed')
		);

		return () => {
			current = false;
		};
	});

	const list = $derived(loaded === null || loaded === 'failed' ? [] : loaded.list);

	const groups = $derived(GROUPS.map((group) => ({ ...group, records: list.filter((record) => record.kind === group.kind) })).filter((group) => group.records.length));
</script>

<FormMessage {form} />

{#if loaded === null}
	<div class="flex flex-col gap-6" aria-busy="true">
		<p class="sr-only">Loading your data</p>
		{#each [3, 2] as rows, index (index)}
			<Card padding="none">
				<div class="flex flex-col gap-1.5 border-b border-line px-5 py-4">
					<span class="flex h-[1lh] items-center text-emphasis"><span class="h-4 w-36 animate-pulse rounded-sm bg-raised"></span></span>
					<span class="flex h-[1lh] items-center text-small"><span class="h-3.5 w-64 max-w-full animate-pulse rounded-sm bg-raised"></span></span>
				</div>
				{#each { length: rows }, row (row)}
					<div class="flex flex-col gap-1 border-b border-line px-5 py-3.5 last:border-b-0">
						<span class="flex h-[1lh] items-center"><span class="h-4 w-1/2 animate-pulse rounded-sm bg-raised"></span></span>
						<span class="flex h-[1lh] items-center text-caption"><span class="h-3 w-24 animate-pulse rounded-sm bg-raised"></span></span>
					</div>
				{/each}
			</Card>
		{/each}
	</div>
{:else if loaded === 'failed'}
	<EmptyState title="Couldn't load your data" body="Something went wrong on our side. Try again in a minute.">
		<Button variant="secondary" size="sm" href="/dashboard/data" data-sveltekit-reload>Try again</Button>
	</EmptyState>
{:else}
	{#each groups as group (group.kind)}
		<section aria-labelledby="held-{group.kind}" class="card">
			<div class="flex items-start justify-between gap-3 border-b border-line px-5 py-4">
				<div class="flex min-w-0 flex-col gap-0.5">
					<h2 id="held-{group.kind}" class="m-0 flex items-baseline gap-2 text-emphasis font-semibold">{group.label}<span class="font-mono text-caption font-normal text-fg-subtle">{group.records.length}</span></h2>
					<p class="m-0 text-small text-fg-muted">{group.hint}</p>
				</div>
				<form method="POST" action="?/deleteAll" class="shrink-0" use:enhance={({ cancel }) => { if (!confirm(`Delete all ${pluralize(group.records.length, 'record')}? This can't be undone.`)) cancel(); }}>
					<input type="hidden" name="kind" value={group.kind} />
					<button type="submit" class="btn-ghost btn-sm" aria-label="Delete all {group.label.toLowerCase()}">Delete all</button>
				</form>
			</div>
			<ul class="m-0 list-none divide-y divide-line p-0">
				{#each group.records as record (record.uri)}
					<li class="flex items-start justify-between gap-3 px-5 py-3.5">
						<div class="flex min-w-0 flex-col gap-0.5">
							<p class="m-0 truncate text-fg">{record.title}</p>
							{#if record.excerpt}<p class="m-0 line-clamp-2 text-small break-words text-fg-muted">{record.excerpt}</p>{/if}
							<time class="text-caption text-fg-subtle" datetime={record.createdAt}>{formatDateTime(record.createdAt)}</time>
						</div>
						<form method="POST" action="?/delete" class="shrink-0" use:enhance={({ cancel }) => { if (!confirm("Delete this? It can't be undone.")) cancel(); }}>
							<input type="hidden" name="uri" value={record.uri} />
							<button type="submit" class="btn-ghost btn-sm" aria-label="Delete {record.title}">Delete</button>
						</form>
					</li>
				{/each}
			</ul>
		</section>
	{:else}
		<EmptyState title="Nothing here" body="Local Network isn't holding anything for you." />
	{/each}
{/if}
