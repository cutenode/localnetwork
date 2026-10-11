// Laylo — laylo.com
//
// A laylo.com/<artist>/<slug> "drop" is a notify-me page: you leave an email
// address or a phone number, and the artist's link reaches you later, by mail
// or by text. There is no file on the page and no request that produces one —
// the only way through is to be sent it. Which means the signup *is* the walk,
// and that is what this does: it reads the drop, and hands the address in
// settings to the artist's list the way the page would.
//
// Reading the drop
// ----------------
// The page is a shell that fetches its own JSON from Laylo's CDN, and that
// JSON says the title, the artist's id, which channels the drop is collecting
// and what else it asks for. A second fetch, also written into the page, gets
// the artist's own record: their display name, and whether they have asked
// Laylo to confirm an address by mail before adding it. Both URLs are read off
// the page rather than guessed at.
//
// A drop of the link kind — where the artist put a destination on it rather
// than a signup — has somewhere to go, and that goes back as a link to open,
// with nothing signed up for.
//
// Signing up
// ----------
// One POST to events.laylo.com, the same one the page's RSVP button makes:
//
//   {type: 'rsvp', payload: {productId, creatorId, email, optIn, dropDate,
//    fingerprintId, …}}  →  {success: true|false}
//
// `creatorId` is the only field the endpoint refuses outright without.
// Everything past it is checked later and out of sight, so `success` means
// taken, not subscribed — which is the most a row can honestly claim, and what
// it says.
//
// The page also attaches a captcha token (reCAPTCHA v3, or Turnstile). That
// field is optional — the endpoint takes a signup carrying none — and a forged
// one is refused, so none is sent rather than one faked. If Laylo ever makes
// it compulsory the signups start coming back refused, and the row goes back
// to offering the drop, which is where it was before any of this.
//
// What it will not do
// -------------------
// A drop collecting only a phone number, one that confirms by text (Laylo
// offers double opt-in over SMS alone), and one asking questions beyond a
// name: this has no phone number to give and no way to answer the rest. Those
// come back as what they always were — a page to open and do yourself.

import { fallback, z } from '$lib/zod';
import { EMAIL, NAME, Wanting } from '../next-step.ts';
import { looseBoolean, looseString } from '../parse.ts';
import { looksLikeFile } from '../save.ts';
import type { Site, UnlockDetails, Unlocked } from './index.ts';

// See channels(). A value that is not a number is a channel not offered.
const RsvpOptions = fallback(z.optional(z.record(z.string(), fallback(z.number(), -1))), undefined);

/** A drop's JSON, as far as it is read here; {} for one that is not an object. */
const Drop = fallback(
	z.object({
		title: looseString,
		/** Where a drop of the link kind points. */
		link: looseString,
		productId: looseString,
		/** Passed on as the drop has it. */
		endDate: z.optional(z.json()),
		/** Whether it confirms signups by text. */
		doubleOptIn: looseBoolean,
		// Any truthy `required` counts, as the page reads it, and an input that is not an object asks for nothing.
		additionalRSVPInputs: fallback(
			z.optional(
				z.array(
					fallback(
						z.object({
							fieldName: fallback(z.string(), ''),
							required: z.pipe(z.unknown(), z.transform(Boolean))
						}),
						{ fieldName: '', required: false }
					)
				)
			),
			undefined
		),
		rsvpOptions: RsvpOptions,
		/** The artist, in brief. */
		user: fallback(z.optional(z.object({ id: looseString, displayName: looseString })), undefined)
	}),
	{}
);

type Drop = z.infer<typeof Drop>;

/** The artist's own record, as far as it is read here; {} where there is none. */
const ArtistRecord = fallback(
	z.object({
		displayName: looseString,
		/** Whether a signup waits on a confirmation mail. */
		requireEmailVerify: looseBoolean,
		/** Overrides the drop's. */
		rsvpOptions: RsvpOptions
	}),
	{}
);

type ArtistRecord = z.infer<typeof ArtistRecord>;

// The CDNs the drop pages have fetched from for as long as they have had one.
// Only fallbacks: the page names both, and that is what is used when it does.
const DROP_CDN = 'https://d21i0hc4hl3bvt.cloudfront.net';

const USER_CDN = 'https://d3oyaxbt9vo0fg.cloudfront.net';

// Where the page's own RSVP button posts.
const SIGNUP = 'https://events.laylo.com/actions/rsvp/rsvp';

/** Where the page says its own drop JSON lives. */
export function dropUrlOf(html: string, pageUrl: string) {
	const found = /fetch\(\s*['"](https:\/\/[^'"]+\.json)['"]/.exec(html);

	if (found) return found[1];
	const path = new URL(pageUrl).pathname.replace(/\/+$/, '');

	return path ? `${DROP_CDN}${path}.json` : '';
}

/**
 * Where the page says the artist's own record lives. The page builds that URL
 * rather than writing it whole — `fetch(<base> + '/users/' + id + '.json')` —
 * so the base is what there is to read off it. The path has also been written
 * onto the end of the base itself, so both shapes count.
 */
export function userUrlOf(html: string, userId: string) {
	const found = /fetch\(\s*['"](https:\/\/[^'"]+?)(?:\/users\/['"]|['"]\s*\+\s*['"]\/users\/['"])\s*\+/.exec(html);

	return `${found ? found[1] : USER_CDN}/users/${userId}.json`;
}

// What the drop calls each channel, as a row should say it.
const CHANNELS = new Map([
	['email', 'an email address'],
	['sms', 'a phone number'],
	['instagram', 'an Instagram follow'],
	['messenger', 'a Messenger message']
]);

/**
 * The channels the drop is collecting, in the order the page offers them. The
 * numbers are that order, not a count: -1 is "not offered at all", and 0 is
 * offered and first. An artist's own options, where they have set any,
 * override the drop's — which is how the page reads them too.
 */
export function channels(drop: Drop, artist: ArtistRecord = {}) {
	const options = artist.rsvpOptions || drop.rsvpOptions || {};

	return Object.entries(options)
		.filter(([, order]) => order > -1)
		.sort(([, one], [, two]) => one - two)
		.map(([channel]) => channel);
}

/** Which of those the row should say it takes. */
export const wants = (drop: Drop, artist?: ArtistRecord) => {
	const asked = channels(drop, artist).map((channel) => CHANNELS.get(channel) ?? channel);

	return asked.length ? asked.join(' or ') : 'a signup';
};

// Laylo's page keeps one of these in a cookie, so the same visitor is
// recognised across every drop they open. There is nothing here to recognise
// and no reason to be recognisable from one artist to the next, so each signup
// gets a fresh one. It is not a credential: any non-empty string is taken.
const fingerprint = () => crypto.randomUUID().replace(/-/g, '');

/** What this drop asks for that the panel has no way to answer. */
function unanswerable(drop: Drop, details: UnlockDetails) {
	if (drop.doubleOptIn) {
		return 'this drop confirms signups by text, so it only takes a phone number.';
	}

	const required = (drop.additionalRSVPInputs || []).filter((input) => input.required);
	const beyond = required.filter((input) => input.fieldName !== 'name');

	if (beyond.length) {
		return `this drop asks for ${beyond.map((input) => input.fieldName).join(' and ')} as well.`;
	}

	// A name it does ask for is one settings holds, so that is where the row
	// sends you rather than to the page.
	if (required.length && !details.name) {
		throw new Wanting(NAME, 'this drop asks for a name with the address — add one in settings.');
	}

	return '';
}

/**
 * Reads one of the files the drop page fetches for itself.
 *
 * @param what what the row should call it
 * @returns its JSON, which the caller parses
 */
async function readJson(url: string, what: string): Promise<z.JSONType> {
	let answer;

	try {
		answer = await fetch(url, { headers: { Accept: 'application/json' } });
	} catch (_e) {
		// The manifest names the hosts Laylo's pages have always fetched from; a
		// page pointing somewhere else is blocked before it is sent.
		throw new Error(
			`Laylo serves this ${what} from ${new URL(url).host}, which the extension can’t ` +
				'reach. open the drop in a tab.',
			{ cause: _e }
		);
	}

	if (!answer.ok) throw new Error(`Laylo answered ${answer.status} for that ${what}.`);

	return answer.json();
}

/** The signup the page's RSVP button posts, as far as this fills it in. */
interface Signup {
	productId?: string;
	creatorId?: string;
	dropDate: z.JSONType;
	email: string;
	optIn: boolean;
	fingerprintId: string;
	/** Both only where settings holds a name. */
	name?: string;
	firstName?: string;
}

/**
 * Hands the address to the artist's list, the way the page's button does.
 * Answers with whether it went on as opted in, or as waiting for the
 * confirmation mail Laylo sends for the artists who ask for one.
 */
async function signUp(drop: Drop, artist: ArtistRecord, details: UnlockDetails) {
	const payload: Signup = {
		productId: drop.productId,
		creatorId: drop.user!.id,
		dropDate: drop.endDate ?? null,
		email: details.email,
		// The page opts a fan straight in unless the artist has asked for a
		// confirmation mail first, which is a flag on their record. An artist
		// whose record would not load is read the way the page reads one that has
		// not arrived yet: as not asking.
		optIn: !artist.requireEmailVerify,
		fingerprintId: fingerprint()
	};

	if (details.name) {
		payload.name = details.name;
		payload.firstName = details.name;
	}

	let answer;

	try {
		answer = await fetch(SIGNUP, {
			method: 'POST',
			headers: { 'Content-Type': 'application/json' },
			body: JSON.stringify({ type: 'rsvp', payload })
		});
	} catch (_e) {
		throw new Error('Laylo’s signup wouldn’t answer. open the drop and sign up there.', { cause: _e });
	}

	if (!answer.ok) throw new Error(`Laylo answered ${answer.status} to the signup.`);
	const said = await answer.json();

	if (!said || !said.success) {
		throw new Error('Laylo turned the signup down. open the drop and sign up there.');
	}

	return payload.optIn;
}

async function unlock(pageUrl: string, details: UnlockDetails): Promise<Unlocked> {
	const page = await fetch(pageUrl, { headers: { Accept: 'text/html' } });

	if (!page.ok) throw new Error(`the drop page returned HTTP ${page.status}.`);
	const html = await page.text();

	const dropUrl = dropUrlOf(html, pageUrl);

	if (!dropUrl) throw new Error('that’s not a Laylo drop.');
	const drop = Drop.parse(await readJson(dropUrl, 'drop'));

	// The drop's own copy of the artist is an id and a username; the name to put
	// on a row — and the one setting the signup turns on — are on their record,
	// which is the second file the page fetches.
	const artist = drop.user?.id ? ArtistRecord.parse(await readJson(userUrlOf(html, drop.user.id), 'artist')) : {};
	const who = artist.displayName || drop.user?.displayName || '';
	const title = [who, drop.title || ''].filter(Boolean).join(' — ');

	// A drop with a destination on it rather than a signup: nothing to sign up
	// for, and somewhere to go.
	if (drop.link) {
		return looksLikeFile(drop.link)
			? { kind: 'file', url: drop.link, title }
			: { kind: 'link', url: drop.link, title, why: 'the drop points here.' };
	}

	const offer = { kind: 'link', url: pageUrl, title } as const;

	// No artist on the drop is no list to be put on: the signup is addressed to
	// one, and the endpoint is the first thing to say so.
	if (!drop.user?.id || !channels(drop, artist).includes('email')) {
		return {
			...offer,
			why:
				`Laylo is a notify-me page, not a gate: it takes ${wants(drop, artist)} and the artist ` +
				'sends the link out. that’s not something this can leave — open it and sign up if you ' +
				'want it.'
		};
	}

	const stopper = unanswerable(drop, details);

	if (stopper) return { ...offer, why: `${stopper} open it and sign up there if you want it.` };

	if (!details.email) {
		throw new Wanting(
			EMAIL,
			'Laylo sends this one out by mail rather than handing it over — add an address in ' +
				'settings and the row signs you up for it.'
		);
	}

	const optedIn = await signUp(drop, artist, details);

	return {
		kind: 'signup',
		url: pageUrl,
		title,
		why: optedIn
			? `signed ${details.email} up — ${who || 'the artist'} sends the link out by mail.`
			: `signed ${details.email} up — confirm it in the mail Laylo sends, and ` +
				`${who || 'the artist'} sends the link out.`
	};
}

export default {
	id: 'laylo',
	name: 'Laylo',
	hosts: ['laylo.com'],
	subfolder: 'Laylo',
	// Almost never a file: a drop is a signup, and only the link kind goes
	// anywhere on its own.
	yields: 'mixed',
	wantsEmail: true,
	unlock
} satisfies Site;
