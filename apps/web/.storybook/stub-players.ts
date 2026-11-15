import type { BrowserContext } from 'playwright';
import type { BrowserCommand } from 'vitest/node';

/** The platform players a story can open (`#lib/mix/links.ts`). */
const PLAYERS = /^https:\/\/(w\.soundcloud\.com|player-widget\.mixcloud\.com)\//;

const stubbed = new WeakSet<BrowserContext>();

/**
 * Answers SoundCloud's and Mixcloud's players with a page that shows the URL it was asked for, so the story tests
 * open players without loading the real widgets. Storybook itself still shows the real ones.
 */
export const stubPlayers: BrowserCommand<[]> = async ({ context }) => {
	if (stubbed.has(context)) return;
	stubbed.add(context);

	await context.route(PLAYERS, (route) =>
		route.fulfill({
			contentType: 'text/html',
			body: `<!doctype html><title>Player stub</title><p>${route.request().url().replaceAll('&', '&amp;')}</p>`
		})
	);
};
