<script lang="ts">
	/**
	 * Asked when a link inside the site would leave a page with unsaved changes. Keep editing is focused first, so Enter
	 * or Escape never throws work away. Leaving the site or reloading gets the browser's own question instead, which is
	 * the only one a browser allows there.
	 */
	let { open, name, onstay, ondiscard, onsave }: { open: boolean; name: string; onstay: () => void; ondiscard: () => void; onsave: () => void } = $props();

	let dialog = $state<HTMLDialogElement>();

	$effect(() => {
		if (open && !dialog?.open) dialog?.showModal();
		else if (!open && dialog?.open) dialog.close();
	});
</script>

<dialog bind:this={dialog} aria-labelledby="leave-title" oncancel={(event) => { event.preventDefault(); onstay(); }} class="m-auto w-[min(34rem,calc(100vw-2rem))] max-w-none rounded-xl border border-line-strong bg-surface p-0 text-fg backdrop:bg-canvas/80">
	<div class="flex flex-col gap-2 p-5">
		<h2 id="leave-title" class="text-emphasis">Leave without saving?</h2>
		<p class="text-small text-fg-soft">Your changes to {name} aren't saved yet. They stay in this browser if you leave, but nobody else sees them.</p>
	</div>
	<div class="flex flex-wrap justify-end gap-2 border-t border-line px-5 py-3 max-sm:[&>*]:flex-1">
		<button type="button" class="btn-ghost btn-sm" onclick={ondiscard}>Discard and leave</button>
		<!-- svelte-ignore a11y_autofocus -->
		<button type="button" class="btn-secondary btn-sm" autofocus onclick={onstay}>Keep editing</button>
		<button type="button" class="btn-primary btn-sm" onclick={onsave}>Save and leave</button>
	</div>
</dialog>
