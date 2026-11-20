<script lang="ts">
	import 'asciinema-player/dist/bundle/asciinema-player.css';
	import { create } from 'asciinema-player';
	import type { Attachment } from 'svelte/attachments';
	import { FONT, fontLoaded } from './terminal.ts';

	/** An asciicast recording of a widget that moves, looping, or held on its first frame for reduced motion. */
	let { cast }: { cast: string } = $props();

	const play: Attachment<HTMLDivElement> = (host) => {
		const player = create({ data: cast }, host, {
			autoPlay: !matchMedia('(prefers-reduced-motion: reduce)').matches,
			loop: true,
			poster: 'npt:0:0',
			fit: false,
			terminalFontFamily: FONT.family,
			terminalFontSize: `${FONT.size}px`,
			terminalLineHeight: FONT.lineHeight
		});

		return () => player.dispose();
	};
</script>

{#await fontLoaded() then}
	<div {@attach play}></div>
{/await}
