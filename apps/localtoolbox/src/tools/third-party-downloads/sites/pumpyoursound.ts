// Pump Your Sound (pumpyoursound.com)
//
// A fangate page (/f/<channel>/<slug>/<id>) is server-rendered PHP with an
// ordinary form in it, `frm-freeDownloadForm`, and pressing its Download button
// submits it; the answer is the file. The steps beside the button (follow on
// SoundCloud or Instagram, leave a comment) are OAuth flows of their own.
//
// The person presses Download on the page: Get opens it and gate-page.ts
// outlines the button. The browser downloads what the form answers with, and
// the background worker files it into PumpYourSound/ and marks the track
// (./on-page.ts).

import type { Site } from './index.ts';

export default {
	id: 'pumpyoursound',
	name: 'Pump Your Sound',
	hosts: ['pumpyoursound.com'],
	subfolder: 'PumpYourSound',
	how: 'page',
	page: {
		say: 'press Download. it saves to PumpYourSound/.',
		steps: '#frm-freeDownloadForm [type="submit"]'
	}
} satisfies Site;
