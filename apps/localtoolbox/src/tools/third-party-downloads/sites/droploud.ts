// Droploud (droploud.com)
//
// (The one people write down as "DropCloud". The site spells it Droploud, and
// so does the SoundCloud link it arrives on.)
//
// A Droploud gate (/gate/<id> or /track/<id>) is a box of steps: an email
// address, follows on Instagram, a SoundCloud connection that follows, likes,
// reposts and comments. Droploud's terms want those done by the person, so the
// gate is done on its page: Get opens it, gate-page.ts outlines the box and
// fills the email field from settings while it is empty, and the person does
// the steps. When the gate unlocks, the page starts the download itself (it
// clicks a link to the file), and the background worker files that download
// into Droploud/ and marks the track (./on-page.ts).

import type { Site } from './index.ts';

export default {
	id: 'droploud',
	name: 'Droploud',
	hosts: ['droploud.com'],
	subfolder: 'Droploud',
	how: 'page',
	page: {
		say: 'do the outlined steps, then press Free download. it saves to Droploud/.',
		steps: 'form.ds-gate-pane',
		email: 'form.ds-gate-pane input[type="email"]'
	}
} satisfies Site;
