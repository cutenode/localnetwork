<script lang="ts">
	import '@xterm/xterm/css/xterm.css';
	import { WebglAddon } from '@xterm/addon-webgl';
	import { Terminal } from '@xterm/xterm';
	import type { Attachment } from 'svelte/attachments';
	import type { Capture } from './captures.ts';
	import { FONT, fontLoaded } from './terminal.ts';

	/** A terminal the size of a captured widget, showing the bytes it was sent. */
	let { bytes, cols, rows, page }: Pick<Capture, 'bytes' | 'cols' | 'rows' | 'page'> = $props();

	/** The screen as text once it's drawn, for screen readers and the story tests. */
	let text = $state<string>();

	const draw: Attachment<HTMLDivElement> = (host) => {
		const terminal = new Terminal({
			cols,
			rows,
			fontFamily: FONT.family,
			fontSize: FONT.size,
			lineHeight: FONT.lineHeight,
			theme: { background: page.bg, foreground: page.fg },
			disableStdin: true,
			scrollback: 0
		});

		terminal.open(host);
		// WebGL draws box drawing, blocks and braille as shapes that fill the cell, not from whichever
		// font on the machine has them, so a widget looks the same everywhere.
		terminal.loadAddon(new WebglAddon());
		terminal.write(bytes, () => {
			terminal.selectAll();
			text = terminal.getSelection();
			terminal.clearSelection();
		});

		return () => terminal.dispose();
	};
</script>

{#await fontLoaded() then}
	<div inert {@attach draw}></div>
{/await}
{#if text !== undefined}
	<pre class="sr-only" data-drawn>{text}</pre>
{/if}
