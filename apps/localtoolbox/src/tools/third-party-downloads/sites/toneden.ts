// ToneDen (toneden.io) and FanLink (fanlink.tv), ToneDen's smart links.
//
// Both are opened and nothing more. A ToneDen gate's step is an OAuth follow
// that ToneDen performs itself, so the gate has to be done on its page anyway,
// and ToneDen shuts down on 2026-12-01, both domains with it. Their links still
// sit on a lot of SoundCloud tracks, so they keep a row: it says which site
// it is and opens the page.
//
// fanlink.to was ToneDen's earlier domain for FanLinks and answers "Brand not
// found" on every path; a .to link opens on .tv (`aliases`, see ./index.ts).

import type { Site } from './index.ts';

const CLOSING = 'ToneDen shuts down on Dec 1, 2026.';

export const toneden = {
	id: 'toneden',
	name: 'ToneDen',
	hosts: ['toneden.io'],
	how: 'link',
	note: CLOSING
} satisfies Site;

export const fanlink = {
	id: 'fanlink',
	name: 'FanLink',
	hosts: ['fanlink.tv'],
	aliases: ['fanlink.to'],
	how: 'link',
	note: CLOSING
} satisfies Site;
