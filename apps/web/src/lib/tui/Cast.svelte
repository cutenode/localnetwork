<script lang="ts">
	import 'asciinema-player/dist/bundle/asciinema-player.css';
	import { create } from 'asciinema-player';
	import type { Attachment } from 'svelte/attachments';
	import { players } from './players.ts';
	import { FONT, fontLoaded } from './terminal.ts';

	/** An asciicast recording of a widget that moves, looping, or held on its first frame for reduced motion. */
	let { cast }: { cast: string } = $props();

	const play: Attachment<HTMLDivElement> = (host) => {
		const player = create({ data: cast }, host, {
			autoPlay: !matchMedia('(prefers-reduced-motion: reduce)').matches,
			loop: true,
			// A poster shows what was sent before its time, and the first frame is sent at 0, so it's a millisecond in.
			poster: 'npt:0.001',
			fit: false,
			terminalFontFamily: FONT.family,
			terminalFontSize: `${FONT.size}px`,
			terminalLineHeight: FONT.lineHeight
		});

		players.set(host, player);

		return () => player.dispose();
	};
</script>

<!-- As wide as the recording, so on a phone it scrolls like a still instead of the player cutting it off, and
     without the player's own border and corners, so it sits where a still does. -->
{#await fontLoaded() then}
	<div data-cast class="w-max [&_.ap-player]:rounded-none! [&_.ap-term]:border-0!" {@attach play}></div>
{/await}
