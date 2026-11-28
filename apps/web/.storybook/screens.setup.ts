import { afterEach, expect, inject } from 'vitest';
import { page } from 'vitest/browser';

declare module 'vitest' {
	interface ProvidedContext {
		/** Whether the stories are drawn in the pinned browser, so their screenshots can be compared. */
		screens: boolean;
	}
}

/** The story's own tags, which the Storybook test runner puts on the test's context. */
type Ran = { story?: { tags: string[] } };

/**
 * After each story's play test, the story as drawn matches the screenshot saved for it and its project,
 * exactly, with animations stopped (a moving TUI story is held on its first frame by its play test).
 * A story whose content can't be the same twice (a clock, random data, video) carries the
 * `no-screenshot` tag, with why beside it. `pnpm test:tui-screens --update` saves them again.
 */
afterEach(async (context) => {
	const { task } = context;

	if (task.result?.state === 'fail' || !inject('screens')) return;

	// SAFETY: the Storybook test runner sets `story` on the context of every story test it runs.
	if ((context as typeof context & Ran).story?.tags.includes('no-screenshot')) return;

	const story =
		document.querySelector<HTMLElement>('[data-tui-story]') ?? document.querySelector<HTMLElement>('#storybook-root');

	if (!story) throw new Error(`${task.name}: no story on the page`);
	await expect.element(page.elementLocator(story)).toMatchScreenshot({ screenshotOptions: { animations: 'disabled' } });
});
