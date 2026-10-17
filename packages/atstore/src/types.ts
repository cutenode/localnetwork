import type { Browser } from 'playwright';

/** A screenshot of the tool, as the PNG it was taken as. */
export interface Shot {
	/** Part of its file name, such as `profile`. */
	name: string;
	png: Buffer;
}

/** How the hero draws its screenshots: in a browser window with its address, a terminal window, or as they are. */
export type Frame = { kind: 'browser'; url: string } | { kind: 'terminal'; title: string } | { kind: 'panel' };

/** A tool's AT Store listing, as far as its pictures go. */
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
		/** Which screenshots the hero shows, by their place in what `capture` returns. */
		shots: number[];
		frame: Frame;
	};
	/** Builds what it needs and takes its screenshots, in the order the listing shows them. */
	capture(browser: Browser): Promise<Shot[]>;
}
