<script lang="ts">
	import { Camera } from '@lucide/svelte';

	/**
	 * A page's picture in edit mode, in the box the picture takes: a dashed square with a camera, which picks a new one
	 * (`avatar`) and shows it at once. A page that has one can also have it taken off (`removeAvatar`). The server checks
	 * the file (parseAvatar).
	 */
	let { src = null, error, readonly = false }: { src?: string | null; error?: string; readonly?: boolean } = $props();

	let picked = $state<string | null>(null);

	function onchange(event: Event & { currentTarget: HTMLInputElement }) {
		const file = event.currentTarget.files?.[0];

		if (picked) URL.revokeObjectURL(picked);
		picked = file ? URL.createObjectURL(file) : null;
	}

	const shown = $derived(picked ?? src);
</script>

<div class="flex shrink-0 flex-col gap-2">
	<label class="relative grid size-16 cursor-pointer place-items-center overflow-hidden rounded-xl border border-dashed border-line-strong text-fg-muted hover:text-fg has-focus-visible:outline-2 has-focus-visible:outline-link">
		{#if shown}<img src={shown} alt="" class="absolute inset-0 size-full object-cover opacity-60" />{/if}
		<Camera aria-hidden="true" class="relative size-5" />
		<span class="sr-only">Picture</span>
		<input type="file" name="avatar" accept="image/png,image/jpeg,image/webp" class="sr-only" disabled={readonly} aria-invalid={error ? 'true' : undefined} {onchange} />
	</label>
	{#if src && !picked}<label class="flex items-center gap-1.5 text-label text-fg-muted"><input type="checkbox" class="checkbox" name="removeAvatar" disabled={readonly} /> Remove</label>{/if}
	{#if error}<span class="text-label text-critical">{error}</span>{/if}
</div>
