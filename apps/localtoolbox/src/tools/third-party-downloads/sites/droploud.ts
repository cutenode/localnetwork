// Droploud — droploud.com
//
// (The one people write down as "DropCloud". The site spells it Droploud, and
// so does the SoundCloud link it arrives on.)
//
// A Droploud link is /gate/<track id> or /track/<track id>, and the page ships
// its whole gate in the HTML: the track, the artist, and a `steps` list saying
// which follows, likes, reposts and saves it asks for. Working it is three
// kinds of request against api.droploud.com:
//
//   POST /gate/<id>/step      {session_id, step}   — one step done
//   POST /gate/<id>/download  {session_id}         — {download_url, type}
//   POST /soundcloud/verify-repost {track_id, session_id, give_up}
//
// The session id is not a credential. The page invents a UUID on first paint,
// keeps it in sessionStorage, and stamps it on everything; the server only uses
// it to tie one visitor's steps together. So a gate walked here is a fresh
// visitor, exactly as a private window would be.
//
// What the gate actually checks
// -----------------------------
// The download post is refused ("Gate not completed", HTTP 403) until the steps
// have been posted, but the steps themselves are self-reported: the page marks
// one done the moment its button is clicked — the click opens the artist's
// SoundCloud or Spotify page in a popup, and nothing checks what happens there.
// Posting the step is worth exactly what clicking the button is worth, which is
// why this tool does it and says so plainly rather than pretending otherwise.
//
// One step is different. A `soundcloud_repost` step is verified for real: it
// wants a SoundCloud account connected by OAuth and then reads that account's
// reposts back. There is nothing to self-report, and nothing here signs in as
// anybody — so the tool takes Droploud's own way out, the "give up" the page
// offers after a couple of failed checks, which drops the step rather than
// claiming it. A gate that will not open without it says so on its row.

import { fallback, z } from '$lib/zod';
import { flightPayload, jsonAfter } from '../parse.ts';
import { looksLikeFile } from '../save.ts';
import type { Site, UnlockDetails, Unlocked } from './index.ts';

/** The page's `gateData`, as far as it is read here. */
const GateData = z.object({
	track: z.object({
		/** The track's, as the page has it. */
		id: z.union([z.string(), z.number()]),
		title: fallback(z.string(), '')
	}),
	artist: fallback(z.object({ display_name: fallback(z.string(), '') }), { display_name: '' }),
	/** Each follow, like, repost or save it asks for; one that can't be read is skipped, not the rest. */
	steps: fallback(
		z.array(fallback(z.nullable(z.object({ id: z.union([z.string(), z.number()]), type: z.string() })), null)),
		[]
	)
});

/**
 * What any of the API's endpoints answers, as far as it is read here. A body
 * that is not JSON, or not an object, says nothing.
 */
const Answer = fallback(
	z.object({
		gave_up: fallback(z.boolean(), false),
		found: fallback(z.boolean(), false),
		download_url: fallback(z.string(), ''),
		error: fallback(z.string(), '')
	}),
	{ gave_up: false, found: false, download_url: '', error: '' }
);

const API = 'https://api.droploud.com/api';

/** The gate as the page carries it: the track, the artist, and the steps. */
export function parseGate(html: string) {
	const gate = GateData.safeParse(jsonAfter(flightPayload(html), '"gateData":'));

	if (!gate.success || !gate.data.track.id) return null;
	const { track, artist } = gate.data;
	const steps = gate.data.steps.flatMap((step) => (step ? [step] : []));

	return { id: track.id, title: track.title, artist: artist.display_name, steps };
}

async function post(path: string, body: Record<string, string | number | boolean>) {
	const response = await fetch(API + path, {
		method: 'POST',
		credentials: 'include',
		headers: { 'Content-Type': 'application/json', Accept: 'application/json' },
		body: JSON.stringify(body)
	});

	let answer = null;

	try {
		answer = await response.json();
	} catch (_e) {
		// A body that is not JSON leaves `answer` null; the status still decides.
	}

	return { ok: response.ok, status: response.status, answer: Answer.parse(answer) };
}

async function unlock(pageUrl: string, _details: UnlockDetails): Promise<Unlocked> {
	const page = await fetch(pageUrl, { credentials: 'include', headers: { Accept: 'text/html' } });

	if (!page.ok) throw new Error(`the gate page returned HTTP ${page.status}.`);

	const gate = parseGate(await page.text());

	if (!gate) throw new Error('that’s not a Droploud gate.');

	// The page's own id for a visit: a UUID, kept for the length of the walk.
	const session = crypto.randomUUID();
	let gaveUpRepost = false;

	for (const step of gate.steps) {
		if (step.type === 'soundcloud_repost') {
			// Not something to self-report: Droploud reads this one back off
			// SoundCloud. Its own escape hatch drops the step instead.
			const { answer } = await post('/soundcloud/verify-repost', {
				track_id: gate.id,
				session_id: session,
				give_up: true
			});

			gaveUpRepost = !(answer.gave_up || answer.found);
			continue;
		}

		// A failed step is not fatal on its own — the download call below is what
		// says whether the gate is satisfied, and it gives the better message.
		await post(`/gate/${gate.id}/step`, { session_id: session, step: step.id });
	}

	const { ok, status, answer } = await post(`/gate/${gate.id}/download`, { session_id: session });

	if (!ok || !answer.download_url) {
		if (gaveUpRepost) {
			throw new Error(
				'Droploud is enforcing the repost step here, which needs a SoundCloud account connected ' +
					'on the page itself. open it in a tab.'
			);
		}

		throw new Error(
			answer.error
				? `Droploud answered "${answer.error}" (HTTP ${status}). open the gate in a tab to see what it wants.`
				: `Droploud answered ${status} to the download. open the gate in a tab to see what it wants.`
		);
	}

	const title = [gate.artist, gate.title].filter(Boolean).join(' — ');

	// `type: "external"` is Droploud saying the artist pointed the gate somewhere
	// else — usually a Dropbox or Drive link, which is a file often enough to be
	// worth looking at rather than assuming either way.
	return looksLikeFile(answer.download_url)
		? { kind: 'file', url: answer.download_url, title }
		: {
				kind: 'link',
				url: answer.download_url,
				title,
				why: 'the gate opened, but the artist pointed it at a page rather than a file.'
			};
}

export default {
	id: 'droploud',
	name: 'Droploud',
	hosts: ['droploud.com'],
	subfolder: 'Droploud',
	yields: 'file',
	wantsEmail: false,
	unlock
} satisfies Site;
