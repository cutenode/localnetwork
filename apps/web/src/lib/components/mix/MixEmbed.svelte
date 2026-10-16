<script lang="ts">
	/** A platform's player for a mix, from `embedUrl`. */
	let { id, src, title }: { id: string; src: string; title: string } = $props();
</script>

<iframe {id} {title} {src} class="block h-40 w-full rounded-lg border border-line bg-well" loading="lazy" allow="autoplay; encrypted-media" referrerpolicy="strict-origin-when-cross-origin"></iframe>
