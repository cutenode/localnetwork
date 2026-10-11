// Hypeddit — hypeddit.com
//
// A Hypeddit link is a "download gate": a slideshow of steps — follow the
// artist on SoundCloud, hand over an email address, open Instagram — with the
// download button at the end.
//
// What the gate actually checks
// -----------------------------
// Most of the social steps are decoration. Their buttons open the artist's
// SoundCloud (or Instagram, or TikTok) page in a popup and take a CSS class off
// themselves; nothing is reported back and nothing is verified, on the page or
// on the server. Pressing Download then posts the gate's own hidden form
// fields to /gate/download/ul, which answers with a signed S3 link to the
// file. So unlocking most gates is: read the fields off the page, post them
// back.
//
// Two steps have teeth, and both are teeth for the same reason — they are what
// the artist is actually being paid in.
//
// The email step. A gate that asks for an address will not unlock until that
// address has been through /verifyEmailAddress, which checks it is real and
// deliverable and records the lead for the artist. So the tool asks for a real
// address rather than inventing one, and only for the gates that want it.
//
// The Spotify step (`sp` in the step list). This one is not reported by the
// page at all: the button sends the visitor through Spotify's own OAuth, and
// Hypeddit's /spotify_callback then does the following and the library-saving
// itself, with a token of its own. /gate/download/ul answers
// {download_status: false, URL: ""} until that has happened, and there is no
// field to say otherwise — claiming a shorter step list, passing
// skip_gate_steps, or posting the artist ids the page carries all get the same
// refusal, because the check is on the session and not on the post. So the
// only way through it is through it: send a tab the same way the button would,
// and let the follow really happen. See connectSpotify below.

import { fallback, z } from '$lib/zod';
import { documentOf } from '../parse.ts';
import { EMAIL, Wanting } from '../next-step.ts';
import type { Site, UnlockDetails, Unlocked } from './index.ts';

/** Never null: unlock stops first. */
type Gate = NonNullable<ReturnType<typeof parseGate>>;

const ORIGIN = 'https://hypeddit.com';

// What a step in `nwSteps` is, for a row that has to say which one stopped it.
// Only the ones a walk can be stopped by are worth naming; anything else falls
// back to its own code.
const STEP_NAMES = new Map([
	['sp', 'Spotify'],
	['dz', 'Deezer'],
	['ap', 'Apple Music'],
	['mc', 'Mixcloud'],
	['email', 'an email address']
]);

// What /verifyEmailAddress answers, as far as it is read here. Anything else
// it might say reads as an address it did not take.
const Verification = fallback(z.object({ attempts: fallback(z.string(), ''), status: fallback(z.string(), '') }), {
	attempts: '',
	status: ''
});

// What /gate/download/ul answers: whether the gate opened, and the signed
// file URL if it did. Anything else reads as a gate that stayed shut.
const Download = fallback(
	z.object({
		download_status: z.pipe(z.unknown(), z.transform(Boolean)),
		URL: fallback(z.string(), '')
	}),
	{ download_status: false, URL: '' }
);

// --- reading a gate page -----------------------------------------------------

function decodeEntities(text: string) {
	return text
		.replace(/&quot;/g, '"')
		.replace(/&#0?39;/g, "'")
		.replace(/&lt;/g, '<')
		.replace(/&gt;/g, '>')
		.replace(/&amp;/g, '&');
}

// The value of a hidden input, found by id. Attribute order and quoting vary
// across gate templates, so match the tag on its id and read `value` out of it.
function valueOf(html: string, id: string) {
	const tag = new RegExp(`<input[^>]*\\bid=["']${id}["'][^>]*>`, 'i').exec(html);
	const value = tag && /\bvalue=["']([^"']*)["']/i.exec(tag[0]);

	return value ? decodeEntities(value[1]) : '';
}

function csrfOf(html: string) {
	const found = /<meta[^>]*name=["']csrf-token["'][^>]*content=["']([^"']*)["']/i.exec(html);

	return found ? found[1] : '';
}

// Where the Spotify step's Connect button would send a tab.
//
// The URL is inside an onclick, wrapped in the page's popup opener, and there
// are several of them on a gate page: the step's own, and the ones in the nav
// and footer that connect an *account* rather than answer this gate. They are
// not interchangeable — each carries an encrypted `state` saying what the
// callback is for — so this takes the one inside the step's own section, and
// nothing at all if that section is not there.
function spotifyConnectOf(html: string) {
	const section = documentOf(html).getElementById('step_sp');

	if (!section) return '';

	for (const element of section.querySelectorAll('[onclick]')) {
		const found = /https:\/\/accounts\.spotify\.com\/authorize\?[^'"\s]+/.exec(element.getAttribute('onclick')!);

		// Already entity-decoded: the attribute came through a parser.
		if (found) return found[0];
	}

	return '';
}

/**
 * The fields the download post needs, as the page carries them.
 *
 * @returns the fields, or null for a page with no gate on it
 */
export function parseGate(html: string) {
	const gate = {
		uid: valueOf(html, 'current_download_file_listner'),
		// A per-page nonce ("<gate id>x<random>") tying the post to this session's
		// view of this gate. A stale one silently refuses to unlock.
		wrndk: valueOf(html, 'wrndk'),
		gvt: valueOf(html, 'gvt'),
		fanGateId: valueOf(html, 'fan_gate_id'),
		steps: valueOf(html, 'nwSteps'),
		isSkippable: valueOf(html, 'is_skippable'),
		duration: Number(valueOf(html, 'duration')) || 0,
		csrf: csrfOf(html),
		spotify: spotifyConnectOf(html)
	};

	return gate.uid && gate.wrndk ? gate : null;
}

// --- talking to Hypeddit -----------------------------------------------------
//
// These run from the panel page, which the manifest gives access to
// hypeddit.com; `credentials: 'include'` is what carries the session cookie
// the gate page set, without which the nonce above means nothing.

/** @returns Hypeddit's JSON answer, whatever the endpoint says */
async function postToGate(path: string, fields: Record<string, string>, csrf: string): Promise<z.JSONType> {
	const response = await fetch(ORIGIN + path, {
		method: 'POST',
		credentials: 'include',
		headers: {
			'Content-Type': 'application/x-www-form-urlencoded; charset=UTF-8',
			'X-Requested-With': 'XMLHttpRequest',
			'X-CSRF-TOKEN': csrf,
			Accept: 'application/json, text/javascript, */*; q=0.01'
		},
		body: new URLSearchParams(fields).toString()
	});

	if (!response.ok) throw new Error(`Hypeddit answered ${response.status} to ${path}.`);

	return response.json();
}

// Hands the address to the gate's email step. Hypeddit answers status "T" once
// it believes the address is real; after too many tries it wants a CAPTCHA,
// which only a real visit to the page can satisfy.
async function verifyEmail(gate: Gate, email: string, name: string) {
	const answer = Verification.parse(
		await postToGate(
			'/verifyEmailAddress',
			{
				validateEmailAddress: email,
				fan_gate_id: gate.fanGateId,
				email_name: name,
				adcode: '',
				hypesource: ''
			},
			gate.csrf
		)
	);

	if (answer.attempts === 'over') {
		throw new Error('Hypeddit wants a CAPTCHA here. open it in a tab, enter your email once, then try again.');
	}

	if (answer.status !== 'T') {
		throw new Wanting(EMAIL, `Hypeddit rejected ${email} — it has to be a real, deliverable address.`);
	}
}

// Answers the Spotify step the only way it can be answered: by going through
// it. The gate's own Connect URL is Spotify's authorize endpoint, carrying the
// scopes for the follow and the library save and an encrypted `state` saying
// which gate is asking; `details.connect` sends a tab out to it and says
// whether the tab came back to Hypeddit, which is where Spotify leaves it once
// the app has been allowed. Coming back is the whole of it — the follow is
// Hypeddit's to do, on its own token, from /spotify_callback.
//
// The first time is not silent: Spotify wants a login, or a yes to the app,
// and a tab opened behind a popup is no place to ask for either. So a trip
// that does not come back is not an error to explain away — it is the row
// saying, once, that this has to be done by hand, after which every later gate
// goes through without a word.
async function connectSpotify(gate: Gate, details: UnlockDetails) {
	const refuse: () => never = () => {
		throw new Error(
			'this gate wants a Spotify follow, and Hypeddit checks that one. open it, press Connect ' +
				'once, and the rest of these unlock here.'
		);
	};

	if (!gate.spotify || !details.connect) refuse();

	// Spotify leaves the tab back on Hypeddit, whichever page of it the callback
	// decides on, so arriving anywhere on the site is the signal.
	if (!(await details.connect(gate.spotify, { landing: `${ORIGIN}/` }))) refuse();
}

/** Walks one gate to the signed file URL. */
async function unlock(pageUrl: string, details: UnlockDetails): Promise<Unlocked> {
	const page = await fetch(pageUrl, { credentials: 'include', headers: { Accept: 'text/html' } });

	if (!page.ok) throw new Error(`the gate page returned HTTP ${page.status}.`);

	const gate = parseGate(await page.text());

	if (!gate) throw new Error('that’s not a Hypeddit gate.');

	// The page pings this on first paint to register the visit; gates that count
	// visits will not unlock without it.
	await postToGate('/gate/ge', { vt: gate.gvt, uid: gate.uid }, gate.csrf);

	const steps = gate.steps ? gate.steps.split(',') : [];
	const wantsEmail = steps.includes('email');

	if (wantsEmail) {
		if (!details.email) {
			throw new Wanting(EMAIL, 'this gate wants an email — add one in settings.');
		}

		await verifyEmail(gate, details.email, details.name);
	}

	// Before the download post rather than after a refusal from it: the follow
	// is per-artist, so a session that walked ten gates is no further through
	// the eleventh, and a post that was never going to work is still a download
	// counted against the artist's gate.
	if (steps.includes('sp')) await connectSpotify(gate, details);

	const answer = Download.parse(
		await postToGate(
			'/gate/download/ul',
			{
				file: gate.uid,
				download_visit: 'true',
				profile_downloads: 'true',
				// Comments the gate can post are stamped at a point inside the track,
				// the way the page picks one.
				time: String(Math.floor(Math.random() * (gate.duration || 180000))),
				sc_comment_text: '',
				yt_comment_text: '',
				page: 'nonsingle',
				is_skippable: gate.isSkippable,
				steps: gate.steps,
				email: wantsEmail ? details.email : '',
				download_action: 'DOWNLOAD',
				wrndk: gate.wrndk,
				is_mobile: '',
				external_id: '',
				hypesource: '',
				adcode: '',
				gvf: '0'
			},
			gate.csrf
		)
	);

	if (!answer.download_status || !answer.URL) {
		// The refusal carries no reason, so the closest thing to one is the gate's
		// own step list — and naming the steps a fetch cannot do saves a trip to
		// the tab to find out the same thing.
		const named = steps.map((step) => STEP_NAMES.get(step)).filter(Boolean);
		throw new Error(
			named.length
				? `the gate wouldn’t unlock — it wants ${named.join(' and ')}. open it in a tab.`
				: 'the gate wouldn’t unlock. open it in a tab to see what it wants.'
		);
	}

	return { kind: 'file', url: answer.URL };
}

export default {
	id: 'hypeddit',
	name: 'Hypeddit',
	hosts: ['hypeddit.com'],
	subfolder: 'Hypeddit',
	// Every gate that unlocks ends in a file.
	yields: 'file',
	wantsEmail: true,
	unlock
} satisfies Site;
