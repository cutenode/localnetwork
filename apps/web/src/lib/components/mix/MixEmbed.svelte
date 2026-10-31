<script lang="ts">
	/** A platform's player for a mix, from `embedUrl`. `element` is the iframe, for its widget API (`#lib/mix/widgets.ts`). */
	let { id, src, title, element = $bindable() }: { id: string; src: string; title: string; element?: HTMLIFrameElement } = $props();
</script>

<iframe bind:this={element} {id} {title} {src} class="block h-40 w-full rounded-lg border border-line bg-well" loading="lazy" allow="autoplay; encrypted-media" referrerpolicy="strict-origin-when-cross-origin"></iframe>
