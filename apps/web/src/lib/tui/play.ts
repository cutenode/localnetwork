import { expect, waitFor } from 'storybook/test';
import { PALETTES } from './captures.ts';
import { players } from './players.ts';

type Play = (context: { canvasElement: HTMLElement }) => Promise<void>;

/**
 * What `selector` finds in `root` once there are `count` of them. It waits on the story's own
 * changes to the page, not a deadline: a terminal draws only after its capture, the font and WebGL
 * have loaded, which on a busy runner takes longer than `waitFor`'s second. The test's timeout
 * still ends a story that never draws.
 */
const shown = (root: HTMLElement, selector: string, count: number) =>
	new Promise<HTMLElement[]>((resolve) => {
		const look = () => {
			const found = [...root.querySelectorAll<HTMLElement>(selector)];

			if (found.length < count) return;

			changes.disconnect();
			resolve(found);
		};

		const changes = new MutationObserver(look);
		changes.observe(root, { childList: true, subtree: true });
		look();
	});

/** A still's play test: both palettes' screens have drawn, and each shows every one of `texts`. */
export const drawn =
	(...texts: string[]): Play =>
	async ({ canvasElement }) => {
		const screens = await shown(canvasElement, '[data-drawn]', PALETTES.length);
		await expect(screens).toHaveLength(PALETTES.length);

		for (const screen of screens) for (const text of texts) await expect(screen.textContent).toContain(text);
	};

/** A moving story's play test: both palettes' recordings play, each frame after the first drawing something else
 *  (a recording stuck on one frame fails), then each is held on its first frame for its screenshot. */
export const playing: Play = async ({ canvasElement }) => {
	// Each host once its player is in it.
	const hosts = (await shown(canvasElement, '[data-cast] .ap-player', PALETTES.length))
		.map((player) => player.closest<HTMLElement>('[data-cast]'))
		.filter((host) => host !== null);

	await expect(hosts).toHaveLength(PALETTES.length);

	// A frame as drawn: its text and SVG glyphs, colours too, as a recording may move a colour, not a character, and
	// the canvas the player paints backgrounds and block glyphs on. Not the `ap-blink` class the player turns on and
	// off every 600 ms to blink blinking text, which isn't a new frame (the recordings hide the cursor, so it never
	// blinks), nor the blank before the first frame, while there's no text or SVG glyph yet.
	const frameOf = (host: HTMLElement) => {
		const term = host.querySelector('.ap-term');
		const painted = term && (term.textContent?.trim() || term.querySelector('use'));

		return painted
			? term.innerHTML.replaceAll(/\s?\bap-blink\b/g, '') + (term.querySelector('canvas')?.toDataURL() ?? '')
			: '';
	};

	// What each has drawn so far; two different frames each show it plays.
	const seen = hosts.map(() => new Set<string>());

	await waitFor(
		async () => {
			for (const [i, host] of hosts.entries()) {
				const frame = frameOf(host);

				if (frame) seen[i].add(frame);
			}

			for (const frames of seen) await expect(frames.size).toBeGreaterThan(1);
		},
		{ timeout: 5000, interval: 50 }
	);

	for (const host of hosts) {
		const player = players.get(host);

		if (!player) throw new Error('a recording with no player');
		await player.pause();
		await player.seek(0);
	}

	// Drawn at the first frame before the screenshot.
	await new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r)));
};
