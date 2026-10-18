<script lang="ts">
	import { artistHref } from '#lib/supports.ts';
	import type { ArtistCredit } from '#lib/types.ts';

	/** A track's artists, each linking to who played their tracks. */
	let { artists }: { artists: ArtistCredit[] } = $props();
</script>

{#each artists as artist, i (i)}{#if i > 0},{' '}{/if}<a class="text-fg-muted underline decoration-line-strong underline-offset-4 hover:text-fg hover:decoration-fg-subtle" href={artistHref(artist)}>{artist.name}</a>{/each}
