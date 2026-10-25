<script lang="ts">
	import EmptyState from '#lib/components/EmptyState.svelte';
	import StatusPill from '#lib/components/StatusPill.svelte';
	import { accountLabel, formatShowDate, timeAgo } from '#lib/format.ts';
	import Button from '#lib/ui/Button.svelte';
	import Card from '#lib/ui/Card.svelte';
	import Monogram from '#lib/ui/Monogram.svelte';
	import SectionHeader from '#lib/ui/SectionHeader.svelte';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();
</script>

<svelte:head><title>Bookings · Local Network</title></svelte:head>

<div class="flex max-w-4xl flex-col gap-8">
	<SectionHeader level={1} eyebrow="Bookings" title="Your booking threads" lede="Each thread is a private space shared only by the promoter and the project's managers." />
	{#if data.threads.length === 0}
		<EmptyState title="No booking threads yet" body="Find a project and send an inquiry with your show details.">
			<Button href="/projects">Find DJs</Button>
		</EmptyState>
	{:else}
		<ul class="m-0 flex list-none flex-col gap-3 p-0">
			{#each data.threads as thread (thread.space)}
				{@const role = thread.participants.find((p) => p.did === data.viewer?.did)?.role}
				<li>
					<Card href="/dashboard/bookings/{thread.skey}" padding="sm" class="flex items-start gap-3.5 sm:items-center sm:p-5">
						<Monogram name={thread.project?.name ?? 'Booking'} />
						<div class="flex min-w-0 flex-1 flex-col gap-2 sm:flex-row sm:items-center sm:justify-between sm:gap-4">
							<div class="flex min-w-0 flex-col gap-0.5">
								<p class="m-0 truncate text-emphasis font-semibold">{thread.title ?? 'Booking inquiry'}</p>
								<p class="m-0 text-small text-fg-muted">{thread.project?.name ?? 'Unknown project'}{thread.showDate ? ` · ${formatShowDate(thread.showDate)}` : ''}</p>
								<p class="m-0 text-caption text-fg-subtle">{role === 'promoter' ? 'You asked' : `From ${accountLabel(thread.promoter)}`} · active {timeAgo(thread.lastActivityAt)}</p>
							</div>
							<div><StatusPill status={thread.status} /></div>
						</div>
					</Card>
				</li>
			{/each}
		</ul>
	{/if}
</div>
