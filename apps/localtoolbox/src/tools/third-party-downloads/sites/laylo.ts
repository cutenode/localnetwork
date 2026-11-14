// Laylo (laylo.com)
//
// Not a download gate: a `laylo.com/<artist>/<slug>` drop is a notify-me page.
// You leave an email address or a phone number, and the artist's link reaches
// you later, by mail or by text. Laylo counts pressing its RSVP button as the
// signature on that sign-up, so the press is the person's: Get opens the drop,
// gate-page.ts fills the email field from settings while it is empty and
// outlines the form, and a press of RSVP is what the row reports. Nothing is
// downloaded here.

import type { Site } from './index.ts';

export default {
	id: 'laylo',
	name: 'Laylo',
	hosts: ['laylo.com'],
	subfolder: 'Laylo',
	how: 'page',
	page: {
		say: 'pick email if it asks for a number, then press RSVP. the artist mails you the link.',
		// The form, and the switch to email on a drop that opens on SMS.
		steps: 'form.laylo-rsvp-form, button[aria-label="RSVP by EMAIL"]',
		email: 'form.laylo-rsvp-form input[type="email"]',
		signup: 'form.laylo-rsvp-form'
	}
} satisfies Site;
