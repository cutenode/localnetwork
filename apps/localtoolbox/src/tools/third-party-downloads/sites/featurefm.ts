// Feature.fm (ffm.to)
//
// A smart link rather than a gate: an ffm.to page is a list of the places a
// release can be played or bought, and the "download" on it is usually iTunes
// or Beatport rather than a file. Choosing one is the person's click on
// Feature.fm's own page, so the row only opens it.

import type { Site } from './index.ts';

export default {
	id: 'featurefm',
	name: 'Feature.fm',
	hosts: ['ffm.to'],
	how: 'link',
	note: 'a page of shop and streaming links.'
} satisfies Site;
