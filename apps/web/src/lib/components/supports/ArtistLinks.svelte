<script lang="ts">
	import { artistHref } from '#lib/supports.ts';
	import type { ArtistCredit } from '#lib/types.ts';

	/** A track's artists, each linking to who played their tracks. */
	let { artists }: { artists: ArtistCredit[] } = $props();
</script>

{#each artists as artist, i (i)}{#if i > 0},{' '}{/if}<a class="entity-link text-fg-muted" href={artistHref(artist)}>{artist.name}</a>{/each}
