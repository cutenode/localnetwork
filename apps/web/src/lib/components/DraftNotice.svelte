<script lang="ts">
	import { History } from '@lucide/svelte';
	import type { FormDraft } from '#lib/form-draft.svelte.ts';
	import { timeAgo } from '#lib/format.ts';

	/** Says a form came back with its unsent changes, and offers to drop them. `files` adds that file inputs come back empty. */
	let { draft, files = false }: { draft: FormDraft<unknown>; files?: boolean } = $props();
</script>

{#if draft.values}
	<div role="status" class="notice notice-info flex items-start gap-2.5">
		<History aria-hidden="true" class="mt-0.5 size-4 shrink-0 text-fg-muted" />
		<!-- Beside the text when it fits, under it on a phone. -->
		<div class="flex min-w-0 flex-1 flex-wrap items-center justify-between gap-x-3 gap-y-1">
			<p class="m-0 min-w-0 flex-1 basis-56">
				Your unsent changes from {timeAgo(draft.savedAt)} are back.{draft.stale ? ' The saved version changed since, so check them before you send.' : ''}{files ? ' Pick any files again.' : ''}
			</p>
			<button type="button" class="btn-ghost btn-sm -my-1" onclick={draft.discard}>Discard</button>
		</div>
	</div>
{/if}
