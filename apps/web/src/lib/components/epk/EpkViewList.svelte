<script lang="ts">
	import { Trash2 } from '@lucide/svelte';
	import { enhance } from '$app/forms';
	import Handle from '#lib/components/Handle.svelte';
	import { type EpkViewReceipt, pressKitPath } from '#lib/epk.ts';
	import { formatDateTime } from '#lib/format.ts';

	/**
	 * Who opened the rest of a project's press kits, for its managers, newest first. Each receipt can be deleted, and
	 * every one goes after `retentionDays`.
	 */
	let { views, slug, retentionDays }: { views: Promise<EpkViewReceipt[]>; slug: string; retentionDays: number } = $props();
</script>

<section aria-labelledby="epk-views" class="flex flex-col gap-3">
	<div class="flex flex-col gap-1">
		<h2 id="epk-views" class="m-0 text-heading font-semibold">Views</h2>
		<p class="m-0 text-small text-fg-muted">Who opened the rest of a press kit. Kept for {retentionDays} days.</p>
	</div>
	{#await views}
		<div role="status" aria-busy="true" aria-label="Loading views" class="flex flex-col gap-2">
			<div class="h-12 animate-pulse rounded-lg bg-surface"></div>
			<div class="h-12 animate-pulse rounded-lg bg-surface"></div>
		</div>
	{:then list}
		{#if list.length}
			<ul class="m-0 flex list-none flex-col p-0">
				{#each list as view (view.id)}
					<li class="flex items-center justify-between gap-3 border-t border-line py-3 last:border-b">
						<p class="m-0 flex min-w-0 flex-col text-small">
							<span class="min-w-0 [overflow-wrap:anywhere]"><Handle account={view.viewer} />{' '}<span class="text-fg-muted">viewed</span>{' '}<a class="link" href={pressKitPath(slug, view.epk.rkey)}>{view.epk.title}</a></span>
							<time class="text-caption text-fg-subtle" datetime={view.viewedAt}>{formatDateTime(view.viewedAt)}</time>
						</p>
						<form method="POST" action="?/forgetView" use:enhance>
							<input type="hidden" name="view" value={view.id} />
							<button class="btn-ghost btn-sm" aria-label="Delete this view of {view.epk.title}"><Trash2 class="size-4" />Delete</button>
						</form>
					</li>
				{/each}
			</ul>
		{:else}
			<p class="m-0 text-small text-fg-muted">No views yet.</p>
		{/if}
	{:catch}
		<p role="alert" class="notice notice-error m-0">Couldn't load the views. Reload the page to try again.</p>
	{/await}
</section>
