import type { PageMeta } from '#lib/meta.ts';

/** A public page's metadata for tests of the page's body, which the root layout writes into the head instead. */
export const PAGE_META: PageMeta = {
	title: 'Page',
	description: 'A page.',
	url: 'https://localnetwork.test/',
	type: 'website'
};
