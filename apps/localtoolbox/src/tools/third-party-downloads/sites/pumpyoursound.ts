// Pump Your Sound — pumpyoursound.com
//
// The oldest-fashioned gate of the set, and the simplest for it. A fangate page
// (/f/<channel>/<slug>/<id>) is server-rendered PHP with an ordinary form in
// it — `frm-freeDownloadForm`, a Nette form, posting back to the page it is on
// with `_do=freeDownloadForm-submit`. Pressing Download submits that form; the
// server answers with the file, or with a redirect to it.
//
// What the gate actually checks
// -----------------------------
// The steps beside the button — follow on SoundCloud, follow on Instagram,
// leave a comment — are separate OAuth flows of their own (/auth/soundcloud-
// events/, /auth/instagram-follow/). They are not part of the form, they set
// no field in it, and the Download button is not disabled while they are
// undone. Submitting the form is exactly what pressing Download does, which is
// all this does.
//
// The comment box is the one field with something in it: the page pre-fills a
// suggested comment. Nothing is posted to SoundCloud by submitting the form —
// the comment only reaches SoundCloud through the OAuth step, which is not
// walked here — so the field goes back as the page had it.

import { documentOf, formFields } from '../parse.ts';
import { looksLikeFile } from '../save.ts';
import type { Site, UnlockDetails, Unlocked } from './index.ts';

const FORM_ID = 'frm-freeDownloadForm';

/**
 * The form's fields and where it posts, as the page carries them.
 *
 * @returns the form, or null for a page without one
 */
export function parseGate(html: string, pageUrl: string) {
	const page = documentOf(html);
	const form = page.getElementById(FORM_ID);

	if (!(form instanceof HTMLFormElement)) return null;

	// The button is not one of the form's values until it is the one pressed, so
	// it goes in by hand — as the page names it, which is how the server tells a
	// download from any other submit.
	const submit = [...form.elements].find(
		(element): element is HTMLInputElement | HTMLButtonElement =>
			(element instanceof HTMLInputElement || element instanceof HTMLButtonElement) &&
			element.type.toLowerCase() === 'submit' &&
			element.name !== ''
	);

	const fields = formFields(form);

	if (submit) fields[submit.name] = submit.value;

	const title = page.querySelector('.fangateTitle');

	return {
		// `form.action` on a form with no action attribute is the page's own URL in
		// a browser, but the empty string in a document that was only parsed.
		action: new URL(form.getAttribute('action') ?? '', pageUrl).href,
		fields,
		title: title ? title.textContent.trim() : ''
	};
}

async function unlock(pageUrl: string, _details: UnlockDetails): Promise<Unlocked> {
	const page = await fetch(pageUrl, { credentials: 'include', headers: { Accept: 'text/html' } });

	if (!page.ok) throw new Error(`the gate page returned HTTP ${page.status}.`);

	const gate = parseGate(await page.text(), pageUrl);

	if (!gate) throw new Error('that’s not a Pump Your Sound fangate.');

	const answer = await fetch(gate.action, {
		method: 'POST',
		credentials: 'include',
		headers: { 'Content-Type': 'application/x-www-form-urlencoded; charset=UTF-8' },
		body: new URLSearchParams(gate.fields).toString()
	});

	if (!answer.ok) throw new Error(`Pump Your Sound answered ${answer.status} to the download.`);

	// The form either redirects at the file — in which case `answer.url` is the
	// file and the body is the file, which there is no reason to read — or comes
	// back as the page again, with the link in it or with a complaint.
	if (answer.url !== gate.action && looksLikeFile(answer.url)) {
		// Not waited on, and a refusal is let go: a body that has already failed
		// rejects the cancel, and there is nothing left of it to stop.
		void Promise.allSettled([answer.body?.cancel?.()]);

		return { kind: 'file', url: answer.url, title: gate.title };
	}

	const html = await answer.text();

	const link = [...documentOf(html).links].map((anchor) => anchor.href).find(looksLikeFile);

	if (link) return { kind: 'file', url: link, title: gate.title };

	throw new Error(
		'Pump Your Sound took the form but handed back nothing. it usually wants the SoundCloud ' +
			'step done in a tab first — open the gate and try there.'
	);
}

export default {
	id: 'pumpyoursound',
	name: 'Pump Your Sound',
	hosts: ['pumpyoursound.com'],
	subfolder: 'PumpYourSound',
	yields: 'file',
	wantsEmail: false,
	unlock
} satisfies Site;
