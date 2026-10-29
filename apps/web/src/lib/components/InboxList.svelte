<script module lang="ts">
	import { Bell, CalendarDays, CalendarPlus, Hourglass, type LucideIcon, Users } from '@lucide/svelte';
	import type { NotificationTopic } from '#lib/types.ts';

	const ICONS: Record<NotificationTopic, LucideIcon> = {
		bookings: CalendarPlus,
		guests: Users,
		waitlist: Hourglass,
		events: CalendarDays
	};
</script>

<script lang="ts">
	import { enhance } from '$app/forms';
	import { formatDateTime, timeAgo } from '#lib/format.ts';
	import type { NotificationView } from '#lib/types.ts';
	import EmptyState from './EmptyState.svelte';

	/**
	 * Someone's inbox, newest first. Each item is a form, so opening one marks it read on the way to what it's about,
	 * with or without JavaScript.
	 */
	let { items }: { items: NotificationView[] } = $props();
</script>

{#if items.length === 0}
	<EmptyState title="Nothing yet" body="Booking inquiries and messages show up here." />
{:else}
	<ol class="card m-0 list-none divide-y divide-line overflow-hidden p-0">
		{#each items as item (item.id)}
			{@const Icon = item.topic ? ICONS[item.topic] : Bell}
			<li>
				<form method="POST" action="?/open" use:enhance>
					<input type="hidden" name="id" value={item.id} />
					<button type="submit" class="flex w-full items-start gap-3.5 px-5 py-4 text-left transition-colors hover:bg-raised">
						<span class="grid size-9 shrink-0 place-items-center rounded-md bg-well text-fg-muted"><Icon class="size-4" aria-hidden="true" /></span>
						<span class="flex min-w-0 flex-1 flex-col gap-0.5">
							<span class={['text-fg', !item.read && 'font-semibold']}>{item.title}</span>
							{#if item.body}<span class="truncate text-small text-fg-muted">{item.body}</span>{/if}
							<time class="text-caption text-fg-subtle" datetime={item.occurredAt} title={formatDateTime(item.occurredAt)}>{timeAgo(item.occurredAt)}</time>
						</span>
						{#if !item.read}<span class="mt-2 size-2 shrink-0 rounded-full bg-link" aria-hidden="true"></span><span class="sr-only">Unread</span>{/if}
					</button>
				</form>
			</li>
		{/each}
	</ol>
{/if}
