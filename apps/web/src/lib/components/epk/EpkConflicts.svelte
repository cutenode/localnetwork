<script lang="ts">
	import { enhance } from '$app/forms';
	import type { EpkConflictView } from '#lib/epk.ts';

	/**
	 * Fields another manager published since this draft changed them, each with the version published now, the one the
	 * draft started from, and the draft's. The manager picks which of theirs to publish; the rest keep the published one.
	 */
	let { conflicts }: { conflicts: EpkConflictView[] } = $props();

	let sending = $state(false);
</script>

<section aria-labelledby="epk-conflicts" class="notice flex flex-col gap-4">
	<div class="flex flex-col gap-1">
		<h2 id="epk-conflicts" class="m-0 text-emphasis font-semibold">Someone published changes first</h2>
		<p class="m-0 text-fg-soft">Pick which of your changes to publish. The rest keep the published version.</p>
	</div>
	<form
		method="POST"
		action="?/resolve"
		class="flex flex-col gap-4"
		use:enhance={() => {
			sending = true;

			return async ({ update }) => {
				await update({ reset: false });
				sending = false;
			};
		}}
	>
		<ul class="m-0 flex list-none flex-col gap-3 p-0">
			{#each conflicts as conflict (conflict.field)}
				<li class="card flex flex-col gap-3 p-4">
					<input type="hidden" name="conflict" value={conflict.field} />
					<h3 class="m-0 text-small font-semibold">{conflict.label}</h3>
					<dl class="m-0 grid gap-3 text-small sm:grid-cols-3">
						<div class="flex min-w-0 flex-col gap-1"><dt class="eyebrow text-fg-subtle">Published now</dt><dd class="m-0 break-words whitespace-pre-line">{conflict.published}</dd></div>
						<div class="flex min-w-0 flex-col gap-1"><dt class="eyebrow text-fg-subtle">When you started</dt><dd class="m-0 break-words whitespace-pre-line text-fg-muted">{conflict.previous}</dd></div>
						<div class="flex min-w-0 flex-col gap-1"><dt class="eyebrow text-fg-subtle">Yours</dt><dd class="m-0 break-words whitespace-pre-line">{conflict.draft}</dd></div>
					</dl>
					<label class="inline-flex w-fit items-center gap-2 text-small font-medium"><input type="checkbox" name="overwrite" value={conflict.field} class="size-4 accent-current" />Publish mine</label>
				</li>
			{/each}
		</ul>
		<div><button class="btn-primary" disabled={sending}>{sending ? 'Publishing…' : 'Publish'}</button></div>
	</form>
</section>
