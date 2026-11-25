<script lang="ts">
	import { enhance } from '$app/forms';
	import { formatShowDate, pluralize, asShowDate } from '#lib/format.ts';

	/** Upcoming events naming a venue that are waiting for the venue, each with Approve, for the people who approve them. */
	let {
		waiting
	}: { waiting: { uri: string; name: string; startsAt: string; party: string | null; lineup: number }[] } = $props();

	type Waiting = (typeof waiting)[number];

	const id = $props.id();

	/** When, on the venue's clock, then the party and how many are on the lineup, where known. */
	const details = (event: Waiting) =>
		[
			formatShowDate(asShowDate(event.startsAt), true),
			event.party,
			event.lineup ? `${pluralize(event.lineup, 'performer')} on the lineup` : null
		]
			.filter(Boolean)
			.join(' · ');
</script>

<section aria-labelledby="{id}-heading" class="card flex flex-col gap-4 p-5 sm:p-6">
	<h2 id="{id}-heading" class="text-emphasis">Waiting for the venue</h2>
	{#if waiting.length}
		<ul class="divide-y divide-line">
			{#each waiting as event (event.uri)}
				<li class="flex flex-wrap items-center justify-between gap-3 py-3">
					<span class="min-w-0">
						<span class="block truncate font-medium text-fg">{event.name}</span>
						<span class="block text-small text-fg-muted">{details(event)}</span>
					</span>
					<form method="POST" action="?/approve" use:enhance>
						<input type="hidden" name="event" value={event.uri} />
						<button class="btn-primary btn-sm" aria-label="Approve {event.name}">Approve</button>
					</form>
				</li>
			{/each}
		</ul>
	{:else}
		<p class="text-small text-fg-muted">Nothing waiting. Events naming this venue show here until you approve them.</p>
	{/if}
</section>
