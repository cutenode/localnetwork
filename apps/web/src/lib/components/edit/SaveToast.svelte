<script lang="ts">
	import { CircleCheck, X } from '@lucide/svelte';

	/**
	 * What a finished save, publish or discard says, always in the same place on every page: the bottom middle of the
	 * window, above the edit bar when there is one. It says it once, politely, and goes after a few seconds, or with its
	 * close button; `undo` adds a button that takes the last step back (a discard). Failures don't toast: they stay in the
	 * bar until they're dealt with.
	 */
	let { message, undo, onclose }: { message: string | null; undo?: () => void; onclose?: () => void } = $props();
</script>

<div class="place pointer-events-none fixed inset-x-0 flex justify-center px-4" role="status" aria-live="polite">
	{#if message}
		<div class="toast flex max-w-md min-w-0 items-center gap-3 rounded-xl border border-line-strong bg-raised py-2 pl-4 text-small text-fg max-sm:w-full">
			<CircleCheck aria-hidden="true" class="size-4 shrink-0 text-good" />
			<span class="min-w-0 flex-1">{message}</span>
			{#if undo}<button type="button" class="btn-ghost btn-sm text-fg" onclick={undo}>Undo</button>{/if}
			<button type="button" class="btn-ghost btn-sm size-9 px-0" aria-label="Close" onclick={onclose}><X aria-hidden="true" class="size-4" /></button>
		</div>
	{/if}
</div>

<style>
	/*
	 * Above the edit bar when there is one (EditBar sets its height). Its own styles rather than utilities only it would
	 * use, so they load with the editor and not in the stylesheet every page loads.
	 */
	.place {
		bottom: calc(var(--edit-bar-space, 0px) + 1rem);
		z-index: 50;
	}
	.toast {
		padding-right: calc(var(--spacing) * 2);
		pointer-events: auto;
		animation: rise 180ms cubic-bezier(0.16, 1, 0.3, 1);
	}
	@keyframes rise {
		from {
			opacity: 0;
			transform: translateY(0.5rem);
		}
	}
	@media (prefers-reduced-motion: reduce) {
		.toast {
			animation-name: fade;
		}
		@keyframes fade {
			from {
				opacity: 0;
			}
		}
	}
</style>
