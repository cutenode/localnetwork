<script lang="ts">
	import EmptyState from '#lib/components/EmptyState.svelte';
	import StatusPill from '#lib/components/StatusPill.svelte';
	import { accountLabel, formatShowDate, timeAgo } from '#lib/format.ts';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();
</script>

<svelte:head><title>Bookings · Local Network</title></svelte:head>

<div class="container-page max-w-4xl py-10">
	<p class="eyebrow">Bookings</p>
	<h1 class="mt-1 text-4xl">Your booking threads</h1>
	<p class="muted mt-2">Each thread is a private atproto space shared only by the promoter and the project's managers.</p>
	<div class="mt-8">
		{#if data.threads.length === 0}
			<EmptyState title="No booking threads yet" body="Find a project and send an inquiry with your show details.">
				<a href="/projects" class="btn-primary">Find DJs</a>
			</EmptyState>
		{:else}
			<ul class="card divide-y-2 divide-dashed divide-ansi-blue">
				{#each data.threads as thread (thread.space)}
					{@const role = thread.participants.find((p) => p.did === data.viewer?.did)?.role}
					<li>
						<a href="/bookings/{thread.skey}" class="flex items-center justify-between gap-4 px-5 py-4 hover:bg-ansi-blue">
							<div class="min-w-0">
								<p class="truncate">{thread.title ?? 'Booking inquiry'} · {thread.project?.name ?? 'Unknown project'}</p>
								<p class="muted text-sm">
									{role === 'promoter' ? 'You asked' : `From ${accountLabel(thread.promoter)}`}{thread.showDate ? ` · show on ${formatShowDate(thread.showDate)}` : ''} · active {timeAgo(thread.lastActivityAt)}
								</p>
							</div>
							<StatusPill status={thread.status} />
						</a>
					</li>
				{/each}
			</ul>
		{/if}
	</div>
</div>
