import type { Browser } from 'playwright';

/** A file `capture` makes: a screenshot such as `player.png`, or data such as `demo-track.json`. */
export interface Capture {
	file: string;
	bytes: Buffer;
}

/** How the hero draws its screenshots: in a browser window with its address, a terminal window, or as they are. */
export type Frame = { kind: 'browser'; url: string } | { kind: 'terminal'; title: string } | { kind: 'panel' };

/** A tool's AT Store listing, as far as its pictures go, and its captures for the website. */
export interface Listing {
	/** Its folder under `apps/`, and its folder of pictures under `out/`. */
	id: string;
	name: string;
	/** The mono line over the name on the hero. */
	eyebrow: string;
	/** The square SVG the icon is drawn from, from the repository root. */
	icon: string;
	/** The colour the hero glows in. */
	glow: string;
	hero: {
		/** Which screenshots the hero shows, by name (`player` is `player.png`). */
		shots: string[];
		frame: Frame;
	};
	/** The screenshots the listing shows, by name, in its order. */
	screenshots: string[];
	/** Whether the tool has a page on the website, which shows everything `capture` makes. */
	page: boolean;
	/** Builds what it needs and makes its captures: the screenshots above, and all its page shows. */
	capture(browser: Browser): Promise<Capture[]>;
}
