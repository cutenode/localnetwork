<script lang="ts">
	import { type BookingGroup, bookingMark } from '#lib/bookings.ts';
	import { accountLabel, timeAgo } from '#lib/format.ts';
	import type { BookingListThread } from '#lib/types.ts';
	import DateBlock from '#lib/ui/DateBlock.svelte';
	import StatusPill from './StatusPill.svelte';

	/**
	 * One group of bookings, each led by its night: dashed until it's confirmed, and "No date" when there isn't one. Who
	 * it waits on is in words, and a pill only says what `group` doesn't. `actsPending` ghosts act names the directory
	 * hasn't answered with yet. `loading` holds the room of that many rows.
	 */
	let {
		threads = [],
		group = 'active',
		viewer = '',
		actsPending = false,
		loading = 0
	}: { threads?: BookingListThread[]; group?: BookingGroup; viewer?: string; actsPending?: boolean; loading?: number } = $props();
</script>

{#snippet act(thread: BookingListThread, fallback = '')}
	{#if thread.project}{thread.project.name}{:else if actsPending}<span class="inline-block h-3 w-20 rounded-md bg-well align-baseline motion-safe:animate-pulse"></span>{:else}{fallback}{/if}
{/snippet}

<ul class="card m-0 list-none divide-y divide-line overflow-hidden p-0" aria-busy={loading > 0 || undefined}>
	{#each { length: loading }, row (row)}
		<li class="flex items-center gap-3 px-4 py-2.5 sm:gap-4 sm:px-5">
			<span class="h-[4.375rem] w-12 shrink-0 rounded-lg bg-well motion-safe:animate-pulse"></span>
			<span class="flex flex-1 flex-col gap-2"><span class="h-4 w-2/5 rounded-md bg-well motion-safe:animate-pulse"></span><span class="h-3 w-3/5 rounded-md bg-well motion-safe:animate-pulse"></span></span>
		</li>
	{/each}
	{#each threads as thread (thread.space)}
		{@const role = thread.participants.find((person) => person.did === viewer)?.role}
		{@const isManager = role === 'manager'}
		{@const hasAct = thread.project || actsPending}
		{@const ago = timeAgo(thread.lastActivityAt)}
		{@const said = thread.lastAuthor === viewer ? `you wrote ${ago}` : ago}
		{@const mark = bookingMark(thread, group, role)}
		<!-- On a phone the pill shares the title's line, which leaves the details the full width under it. -->
		<li class="relative grid grid-cols-[auto_minmax(0,1fr)_auto] items-center gap-x-3 gap-y-0.5 px-4 py-2.5 transition-colors hover:bg-raised sm:gap-x-4 sm:px-5">
			<div class="row-span-2">
				{#if thread.showDate}
					<DateBlock date={thread.showDate.slice(0, 10)} tentative={thread.status !== 'confirmed'} compact />
				{:else}
					<span class="flex h-[4.375rem] w-12 items-center rounded-lg border border-dashed border-line-strong px-2 text-center text-caption text-fg-subtle">No date</span>
				{/if}
			</div>
			<a href="/dashboard/bookings/{thread.skey}" class={['self-end text-body font-medium text-fg no-underline after:absolute after:inset-0', !mark && 'col-[2/-1]']}>{thread.title ?? 'Booking inquiry'}</a>
			{#if mark}<div class="self-end sm:row-span-2 sm:self-center"><StatusPill status={mark} /></div>{/if}
			<p class="col-[2/-1] row-start-2 m-0 self-start text-label text-fg-muted first-letter:uppercase sm:col-[2/3]">
				{#if group === 'active' && isManager}
					Waiting on {accountLabel(thread.promoter)}{#if hasAct}{' · '}{@render act(thread)}{/if}{' · '}{said}
				{:else if group === 'active'}
					Waiting on {@render act(thread, 'the act')}{' · '}{said}
				{:else}
					{#if hasAct}{@render act(thread)}{' · '}{/if}{isManager ? `from ${accountLabel(thread.promoter)}` : 'you asked'}{' · '}{ago}
				{/if}
			</p>
		</li>
	{/each}
</ul>
