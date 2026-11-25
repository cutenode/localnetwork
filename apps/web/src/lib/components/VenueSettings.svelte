<script lang="ts">
	import { enhance } from '$app/forms';
	import { BLOCK_KINDS, labelFor, VENUE_APPROVALS } from '#lib/taxonomy/options.ts';
	import type { VenueApproval } from '#lib/types.ts';

	/**
	 * A venue's settings for its owners: how events that name it show (with page.edit), and who it blocks (with
	 * venue.block). Each section only shows to someone who can change it. `values` refills the block form after a refusal.
	 */
	let {
		approval,
		can,
		blocks,
		values = {}
	}: {
		approval: VenueApproval;
		can: { edit: boolean; block: boolean };
		blocks: { rkey: string; kind: string; name: string }[];
		values?: { kind?: string; name?: string };
	} = $props();

	const id = $props.id();
</script>

<div class="flex flex-col gap-6">
	{#if can.edit}
		<section aria-labelledby="{id}-events" class="card flex flex-col gap-4 p-5 sm:p-6">
			<h2 id="{id}-events" class="text-emphasis">Events that name this venue</h2>
			<form method="POST" action="?/approval" use:enhance class="flex flex-col gap-4">
				<fieldset class="flex flex-col gap-3">
					<legend class="sr-only">How events show</legend>
					{#each VENUE_APPROVALS as option (option.value)}
						<label class="flex items-start gap-2.5">
							<input type="radio" class="checkbox mt-1 rounded-full" name="approval" value={option.value} checked={approval === option.value} />
							<span class="flex flex-col">
								<span class="text-body text-fg">{option.label}</span>
								<span class="text-small text-fg-muted">{option.hint}</span>
							</span>
						</label>
					{/each}
				</fieldset>
				<button class="btn-primary btn-sm self-start">Save</button>
			</form>
		</section>
	{/if}

	{#if can.block}
		<section aria-labelledby="{id}-blocked" class="card flex flex-col gap-4 p-5 sm:p-6">
			<div class="flex flex-col gap-1">
				<h2 id="{id}-blocked" class="text-emphasis">Blocked</h2>
				<p class="text-small text-fg-muted">Their events here stay waiting for the venue, and their links to it and inquiries naming it don't show. Nobody's told.</p>
			</div>
			{#if blocks.length}
				<ul class="divide-y divide-line">
					{#each blocks as block (block.rkey)}
						<li class="flex items-center justify-between gap-3 py-2.5">
							<span class="min-w-0">
								<span class="block truncate text-fg">{block.name}</span>
								<span class="block text-small text-fg-muted">{labelFor(BLOCK_KINDS, block.kind)}</span>
							</span>
							<form method="POST" action="?/unblock" use:enhance>
								<input type="hidden" name="rkey" value={block.rkey} />
								<button class="btn-ghost btn-sm" aria-label="Unblock {block.name}">Unblock</button>
							</form>
						</li>
					{/each}
				</ul>
			{:else}
				<p class="text-small text-fg-muted">Nobody's blocked.</p>
			{/if}
			<form method="POST" action="?/block" use:enhance class="grid gap-3 border-t border-line pt-4 sm:grid-cols-[9rem_minmax(0,1fr)_auto] sm:items-end">
				<div>
					<label class="label" for="{id}-kind">Block a</label>
					<select id="{id}-kind" name="kind" class="input">
						{#each BLOCK_KINDS as kind (kind.value)}
							<option value={kind.value} selected={values.kind === kind.value}>{kind.label}</option>
						{/each}
					</select>
				</div>
				<div>
					<label class="label" for="{id}-name">Handle or link</label>
					<input id="{id}-name" name="name" class="input" required maxlength="300" placeholder="@handle, or an act's or party's link" value={values.name ?? ''} />
				</div>
				<button class="btn-secondary">Block</button>
			</form>
		</section>
	{/if}
</div>
