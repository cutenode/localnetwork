import type { JsonLd } from '#lib/structured-data.ts';

/** The site's name: after each page's title, and what link previews name the site. */
export const SITE_NAME = 'Local Network';

/** The picture a link preview shows for a page. */
export type PageImage = {
	/** An absolute URL. */
	url: string;
	/** What's in the picture, for people who can't see it. */
	alt: string;
	/** Known for the cards the site draws (1200×630); a stored picture's size isn't. */
	width?: number;
	height?: number;
};

/**
 * What a public page tells link previews and search engines about itself. Its server load returns one and the root
 * layout writes it into the head, so it's in the HTML crawlers read: they run no JavaScript, so nothing streamed or
 * set in the browser reaches them.
 */
export type PageMeta = {
	/** The page's own name, without the site's. */
	title: string;
	/** The `<title>`, when it isn't the title followed by the site's name. */
	documentTitle?: string;
	description: string;
	/** The page's canonical address, absolute. */
	url: string;
	/** The AT Protocol records behind the page, as at-tags: DID-based `at://` URIs (see server/meta-records.ts). */
	records?: { canonical: string[]; alternate: string[]; author: string[] };
	type: 'website' | 'profile' | 'music.song';
	image?: PageImage;
	/** Keeps search engines from listing a page only its managers and staff can see. */
	noindex?: boolean;
	/** What the page is about as schema.org structured data, which search engines read for richer results. */
	jsonLd?: JsonLd;
};
