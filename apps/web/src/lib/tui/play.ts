import { expect } from 'storybook/test';

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
		const screens = await shown(canvasElement, '[data-drawn]', 2);
		await expect(screens).toHaveLength(2);

		for (const screen of screens) for (const text of texts) await expect(screen.textContent).toContain(text);
	};

/** A moving story's play test: both palettes' recordings are loaded in a player. */
export const playing: Play = async ({ canvasElement }) => {
	await expect(await shown(canvasElement, '.ap-player', 2)).toHaveLength(2);
};
