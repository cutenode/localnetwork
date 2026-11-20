import { expect, waitFor } from 'storybook/test';

type Play = (context: { canvasElement: HTMLElement }) => Promise<void>;

/** A still's play test: both palettes' screens have drawn, and each shows every one of `texts`. */
export const drawn =
	(...texts: string[]): Play =>
	async ({ canvasElement }) => {
		const screens = await waitFor(async () => {
			const found = [...canvasElement.querySelectorAll<HTMLElement>('[data-drawn]')];
			await expect(found).toHaveLength(2);

			return found;
		});

		for (const screen of screens) for (const text of texts) await expect(screen.textContent).toContain(text);
	};

/** A moving story's play test: both palettes' recordings are loaded in a player. */
export const playing: Play = async ({ canvasElement }) => {
	await waitFor(() => expect(canvasElement.querySelectorAll('.ap-player')).toHaveLength(2));
};
