<script lang="ts">
	import { enhance } from '$app/forms';
	import { formatDateTime, timeAgo } from '#lib/format.ts';
	import type { InboxSubjectView } from '#lib/types.ts';
	import Monogram from '#lib/ui/Monogram.svelte';

	/**
	 * Someone's inbox, a row per booking or other subject, the latest first. Each row is a form, so opening one marks
	 * its subject read on the way there, with or without JavaScript. `loading` holds the room of that many rows.
	 */
	let { rows = [], loading = 0 }: { rows?: InboxSubjectView[]; loading?: number } = $props();
</script>

<ol class={['card m-0 list-none divide-y divide-line overflow-hidden p-0', !loading && 'list-in']} aria-busy={loading > 0 || undefined}>
	{#each { length: loading }, row (row)}
		<li class="flex items-center gap-3 px-4 py-3 sm:gap-4 sm:px-5 sm:py-3.5">
			<span class="size-8 shrink-0 rounded-[9px] bg-well motion-safe:animate-pulse"></span>
			<span class="flex flex-1 flex-col gap-2"><span class="h-4 w-2/5 rounded-md bg-well motion-safe:animate-pulse"></span><span class="h-3 w-3/4 rounded-md bg-well motion-safe:animate-pulse"></span></span>
			<span class="h-3 w-10 shrink-0 rounded-md bg-well motion-safe:animate-pulse"></span>
		</li>
	{/each}
	{#each rows as row (row.id)}
		<li>
			<form method="POST" action="?/open" use:enhance>
				<input type="hidden" name="id" value={row.id} />
				<button type="submit" class="flex w-full items-start gap-3 px-4 py-3 text-left transition-colors hover:bg-raised focus-visible:bg-raised sm:gap-4 sm:px-5 sm:py-3.5">
					<Monogram name={row.scope} size="sm" />
					<span class="flex min-w-0 flex-1 flex-col gap-0.5">
						<span class={['text-small', row.unread ? 'font-semibold text-fg' : 'text-fg-muted']}>{row.title}</span>
						<span class={['line-clamp-2 text-label', row.unread ? 'text-fg-soft' : 'text-fg-muted']}>{row.summary}</span>
					</span>
					<span class="flex shrink-0 items-center gap-2 pt-0.5">
						{#if row.unread}<span class="size-2 rounded-full bg-link" aria-hidden="true"></span><span class="sr-only">Unread</span>{/if}
						<time class="text-caption text-fg-muted tabular-nums" datetime={row.occurredAt} title={formatDateTime(row.occurredAt)}>{timeAgo(row.occurredAt, undefined, 'narrow')}</time>
					</span>
				</button>
			</form>
		</li>
	{/each}
</ol>

<style>
	/* Rows fade in over the placeholders they replace, in the same room, so nothing moves. */
	@media (prefers-reduced-motion: no-preference) {
		.list-in {
			animation: list-in 180ms ease-out;
		}
	}

	@keyframes list-in {
		from {
			opacity: 0;
		}
	}
</style>
