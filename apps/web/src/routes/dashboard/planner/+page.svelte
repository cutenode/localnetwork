<script lang="ts">
	import { enhance } from '$app/forms';
	import { Lock, Plus, Trash2 } from '@lucide/svelte';
	import EmptyState from '#lib/components/EmptyState.svelte';
	import FormMessage from '#lib/components/FormMessage.svelte';
	import PerformerCard from '#lib/components/PerformerCard.svelte';
	import StatusPill from '#lib/components/StatusPill.svelte';
	import { formatShowDate } from '#lib/format.ts';
	import Button from '#lib/ui/Button.svelte';
	import Card from '#lib/ui/Card.svelte';
	import Monogram from '#lib/ui/Monogram.svelte';
	import SectionHeader from '#lib/ui/SectionHeader.svelte';
	import type { PageProps } from './$types';

	let { data, form }: PageProps = $props();
</script>

<svelte:head><title>Planner · Local Network</title></svelte:head>

<div class="flex flex-col gap-12">
	<div class="flex flex-col gap-4">
		<p class="inline-flex items-center gap-1.5 text-caption text-fg-subtle"><Lock class="size-3.5" />Private to you</p>
		<SectionHeader level={1} title="Your planner" lede="Saved DJs and the shows you're booking. It lives in a personal space on your own PDS, and Local Network keeps no copy." />
	</div>
	<FormMessage {form} />

	{#if !data.available}
		<EmptyState title="Planner unavailable" body={'problem' in data && data.problem ? data.problem : "Your PDS doesn't support atproto spaces yet."} />
	{:else}
		<section aria-labelledby="shows" class="flex flex-col gap-5">
			<div class="flex items-center justify-between gap-4">
				<h2 id="shows" class="text-heading">Shows</h2>
				<Button variant="secondary" size="sm" href="/dashboard/planner/shows/new"><Plus class="size-4" />New show</Button>
			</div>
			{#if data.shows.length === 0}
				<EmptyState title="No shows planned" body="Plan a night and build a lineup, or use the matcher to start one."><Button href="/match">Open the matcher</Button></EmptyState>
			{:else}
				<div class="grid gap-5 md:grid-cols-2">
					{#each data.shows as show (show.rkey)}
						<Card as="article" padding="none" class="flex flex-col">
							<div class="flex items-start justify-between gap-3 p-5">
								<div class="flex min-w-0 flex-col gap-1">
									<a class="text-heading text-fg no-underline hover:text-link" href="/dashboard/planner/shows/{show.rkey}">{show.details.title}</a>
									<p class="text-small text-fg-muted">{show.details.date ? formatShowDate(show.details.date) : 'No date yet'}{show.details.venue ? ` · ${show.details.venue}` : ''}</p>
								</div>
								<form method="POST" action="?/deleteShow" use:enhance={({ cancel }) => { if (!confirm('Delete this show?')) cancel(); }}>
									<input type="hidden" name="rkey" value={show.rkey} />
									<button class="btn-ghost btn-sm px-2" aria-label="Delete show"><Trash2 class="size-4" /></button>
								</form>
							</div>
							<ol class="flex flex-col divide-y divide-line border-t border-line">
								{#each show.lineup as slot (slot.project)}
									{@const name = data.performers[slot.project]?.name ?? 'No longer listed'}
									<li class="flex items-center gap-3 px-5 py-3">
										<Monogram {name} size="sm" />
										<div class="flex min-w-0 flex-1 flex-col">
											<span class="truncate text-fg">{name}</span>
											{#if slot.slot}<span class="font-mono text-caption text-fg-subtle">{slot.slot}</span>{/if}
										</div>
										<StatusPill status={slot.status ?? 'considering'} />
									</li>
								{:else}
									<li class="px-5 py-4 text-small text-fg-muted">No lineup yet</li>
								{/each}
							</ol>
						</Card>
					{/each}
				</div>
			{/if}
		</section>

		<section aria-labelledby="saved" class="flex flex-col gap-5">
			<h2 id="saved" class="text-heading">Saved DJs</h2>
			{#if data.saves.length === 0}
				<EmptyState title="Nothing saved" body="Tap Save on a DJ's page to keep them here." />
			{:else}
				<div class="grid gap-5 sm:grid-cols-2 lg:grid-cols-3">
					{#each data.saves as save (save.rkey)}
						{@const performer = data.performers[save.project]}
						{#if performer}
							<PerformerCard {performer}>
								<form method="POST" action="?/unsave" use:enhance>
									<input type="hidden" name="project" value={save.project} />
									<button class="btn-ghost btn-sm">Remove</button>
								</form>
							</PerformerCard>
						{/if}
					{/each}
				</div>
			{/if}
		</section>
	{/if}
</div>
