<script lang="ts">
	import { ExternalLink } from '@lucide/svelte';
	import { musicBrainzUrl } from '#lib/supports.ts';

	/**
	 * The small icon after an artist or track name that opens it on MusicBrainz. The icon keeps `size` inside a 24px
	 * target, whose negative margins keep the line as tall as the text around it.
	 */
	let { kind, mbid, name, size = 14 }: { kind: 'artist' | 'recording'; mbid: string; name: string; size?: number } = $props();
</script>

<a
	class="-my-1 inline-flex size-6 items-center justify-center self-center text-fg-subtle hover:text-fg"
	href={musicBrainzUrl(kind, mbid)}
	rel="noopener noreferrer"
	target="_blank"
	aria-label="{name} on MusicBrainz"><ExternalLink {size} /></a
>
