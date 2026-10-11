// ToneDen — toneden.io, and its smart links at fanlink.tv
//
// Two products on one backend, which is why they share a file.
//
// A **toneden.io/<artist>/post/<slug>** link is a download gate. Unlike
// Hypeddit's, its steps are real: the button says "Follow on SoundCloud" (or
// Spotify, or Twitch) and leads to /auth/<platform>, an OAuth round trip that
// ends with ToneDen holding a token for that account. It then performs the
// follow itself and opens the gate for the fan it just met.
// GET /api/v1/attachments/<id>/download answers no to anyone else — so there
// is nothing to walk here, and this tool does not try. What it does instead:
//
//   - Reads the post through ToneDen's own public lookup, which needs no
//     account, so a row can say what the gate is and what it wants.
//   - Gates whose `download_source` is "redirect" are not files behind a login
//     at all: the artist pointed them at a Dropbox link or a shop, and that
//     destination is right there in the post. Those the tool can finish.
//   - For the rest it asks for the download with whatever ToneDen session the
//     browser already has. If you have followed this artist through ToneDen
//     once, in this browser, that is a real unlocked gate and the file comes
//     back. If not, the row says which account the gate wants and that it has
//     to be connected once — which is the artist's side of the bargain, and
//     not something to fake.
//
// A refusal is not one shape. Nobody signed in at all gets 401 "Please login
// to your account to continue."; a browser ToneDen *does* know, whose fan has
// not been through this particular gate, gets something else — a 400, and its
// own sentence about it. So the answer is read rather than the status alone,
// and a download that comes back no is followed by the gate's own metadata,
// which is public: `is_gate_open` says whether this browser is through, and
// `viewer_id` whether ToneDen knows it at all. That is the difference between
// a row saying "answered 400" and a row saying which account to connect.
//
// A **fanlink.tv/<slug>** link is a smart link: a chooser pointing at Spotify,
// SoundCloud, Beatport and so on, with no file of its own. The page ships the
// whole thing as `window.preloadLink`, so the tool reads the destinations out
// and offers the one worth opening. A fanlink with an unlock attached to it
// (`rsvp_attachment`) is a gate, and goes down the path above.

import { fallback, z } from '$lib/zod';
import { jsonAfter, looseBoolean, looseString } from '../parse.ts';
import { looksLikeFile } from '../save.ts';
import type { Site, Unlocked } from './index.ts';

/** A post's or a FanLink's id, which only ever goes back into a URL. */
const Id = z.union([z.number(), z.string()]);

/** A post, or the unlock a FanLink carries, as far as it is read here. */
const Post = z.object({
	id: Id,
	title: looseString,
	author: fallback(z.optional(z.object({ display_name: looseString })), undefined),
	/** The account a one-platform gate wants. */
	platform: looseString,
	/** A multi-action gate's. */
	platforms: fallback(z.optional(z.array(fallback(z.string(), ''))), undefined),
	multi_action_enabled: looseBoolean,
	/** Whether any one of `platforms` is enough. */
	is_or_condition: looseBoolean,
	/** 'download', or a kind of gate with no file behind it. */
	unlock_type: looseString,
	skip_download: looseBoolean,
	/** 'redirect' where the artist pointed it elsewhere. */
	download_source: looseString,
	direct_url: looseString,
	/** A URL, or the file's name. */
	download_display_url: looseString
});

type Post = z.infer<typeof Post>;

/** A FanLink as the page carries it (`window.preloadLink`), as far as it is read here. */
const FanLink = z.object({
	id: Id,
	title: looseString,
	author: looseString,
	/** Not always a web address. */
	target_url: looseString,
	rsvp_attachment: fallback(z.optional(Post), undefined),
	services: fallback(z.optional(z.array(z.object({ url: looseString, service_name: looseString }))), undefined)
});

type FanLink = z.infer<typeof FanLink>;

// ToneDen's answer to a post lookup; a post it could not read is none at all.
const Lookup = fallback(z.object({ post: fallback(z.nullable(Post), null) }), { post: null });

// ToneDen's errors, as far as they are read here: '' for what one does not say.
const Refusal = fallback(z.object({ message: fallback(z.string(), ''), name: fallback(z.string(), '') }), {
	message: '',
	name: ''
});

// A gate's public metadata: whether this browser is through it, and who
// ToneDen takes the browser for (null for nobody).
const GateMetadata = fallback(
	z.object({
		metadata: fallback(z.nullable(z.object({ is_gate_open: looseBoolean })), null),
		// Any id it gives, number or string; none (or 0, or '') is nobody.
		viewer_id: fallback(
			z.pipe(
				z.nullable(z.union([z.number(), z.string()])),
				z.transform((id) => id || null)
			),
			null
		)
	}),
	{ metadata: null, viewer_id: null }
);

// Where a download answer puts the file. The shape is not documented and has
// moved around, so any of these names will do, and so will a bare string.
const Located = z.object({
	url: looseString,
	URL: looseString,
	download_url: looseString,
	downloadUrl: looseString
});

const DownloadAnswer = fallback(
	z.union([
		z.string(),
		z.pipe(
			Located,
			z.transform(
				(answer: z.infer<typeof Located>) => answer.url || answer.URL || answer.download_url || answer.downloadUrl || ''
			)
		)
	]),
	''
);

const ORIGIN = 'https://www.toneden.io';

const SIGN_IN =
	'ToneDen only hands this file to an account that’s been through the gate. open the link in a ' +
	'tab, connect the account once, then run the row again — the browser keeps the session, and ' +
	'later gates by the same route go straight through.';

// What a refusal should say. ToneDen answers errors as
// {error, message, name}, and its own sentence beats a status code every time
// — except for "not signed in", which it says in API English and which has an
// answer of its own.
/** @param body the answer's JSON, if it had any */
function refused(status: number, body: z.JSONType | null, path: string) {
	const { message, name } = Refusal.parse(body);
	const said = message.trim();
	const anonymous = status === 401 || status === 403 || name === 'notAuthenticated';

	if (anonymous) return new Error(SIGN_IN);

	return new Error(said ? `ToneDen said: ${said}` : `ToneDen answered ${status} to ${path}.`);
}

async function api(path: string) {
	const response = await fetch(ORIGIN + path, {
		credentials: 'include',
		headers: { Accept: 'application/json' }
	});

	// Read the body either way: a refusal carries the reason, and the reason is
	// the whole point of asking.
	const body: z.JSONType | null = await response.json().catch(() => null);

	if (!response.ok) throw refused(response.status, body, path);

	return { response, body };
}

/**
 * The two path segments a post link is made of: /<author>/post/<link path>.
 *
 * @returns null for a link that is not a post
 */
export function postPathOf(url: string): { author: string; linkPath: string } | null {
	const parts = new URL(url).pathname.split('/').filter(Boolean);

	return parts.length >= 3 && parts[1] === 'post' ? { author: parts[0], linkPath: parts.slice(2).join('/') } : null;
}

// The gate as ToneDen describes it, without an account.
async function lookupPost(url: string): Promise<Post> {
	const path = postPathOf(url);

	if (!path) throw new Error('that’s not a ToneDen post link.');

	const query = new URLSearchParams({
		author_username: path.author,
		link_path: path.linkPath
	});

	const { body } = await api(`/api/v1/posts?${query}`);
	const { post } = Lookup.parse(body);

	if (!post || !post.id) throw new Error('ToneDen doesn’t know that post.');

	return post;
}

// The account a gate asks for, in the words its own button uses. Anything not
// listed is a platform ToneDen added since, and reads well enough unhyphenated.
const SERVICE = new Map([
	['apple-music', 'Apple Music'],
	['bandcamp', 'Bandcamp'],
	['bandsintown', 'Bandsintown'],
	['beatport', 'Beatport'],
	['deezer', 'Deezer'],
	['facebook', 'Facebook'],
	['instagram', 'Instagram'],
	['mixcloud', 'Mixcloud'],
	['patreon', 'Patreon'],
	['soundcloud', 'SoundCloud'],
	['spotify', 'Spotify'],
	['spotify-pre-save', 'Spotify'],
	['spotify-save', 'Spotify'],
	['twitch', 'Twitch'],
	['twitter', 'Twitter'],
	['youtube', 'YouTube']
]);

const serviceName = (platform: string) => SERVICE.get(platform) ?? platform.replace(/-/g, ' ');

/**
 * The accounts a gate wants connected.
 *
 * One gate is one platform; a multi-action gate lists several in `platforms`
 * and `is_or_condition` says whether any one of them is enough.
 */
function wanted(post: Post) {
	const many = ((post.multi_action_enabled && post.platforms) || []).filter(Boolean);
	const platforms = many.length || !post.platform ? many : [post.platform];

	return platforms.map(serviceName);
}

// The sentence for a gate this browser is not through. `viewer` is ToneDen's
// own answer to "do you know whoever is asking" — null for nobody at all,
// which is a different fix from a fan who simply has not done this one.
function closedGate(post: Post, viewer: number | string | null) {
	const names = wanted(post);

	// A gate whose platform is "email" opens for any fan who has left an address
	// with ToneDen: there is no account to connect, only a form on the page.
	if (names.length === 1 && post.platform === 'email') {
		return (
			'this gate only wants an email address, given on ToneDen’s own page. open the link, leave ' +
			'one, then run the row again.'
		);
	}

	const list = names.length ? names.join(post.is_or_condition ? ' or ' : ' and ') : 'an account of its own';

	return viewer
		? `the ToneDen account this browser is signed in to hasn’t been through this gate: it wants ` +
				`${list}. open the link, connect once, then run the row again.`
		: `this gate wants ${list}, and ToneDen doesn’t know this browser. open the link, connect ` +
				'once, then run the row again — the session sticks, and later gates by the same route go ' +
				'straight through.';
}

/**
 * Why a download came back no.
 *
 * The gate's metadata is public, so the reason can be looked up even though
 * the file cannot: `is_gate_open` for whether this browser is through it, and
 * `viewer_id` for whether ToneDen knows this browser at all. A gate that is
 * open and still refused is not a gate problem, so that answer is left alone.
 *
 * @param cause the download's own refusal
 */
async function whyRefused(post: Post, cause: unknown) {
	let answer;

	try {
		answer = await api(`/api/v1/attachments/${post.id}/gateMetadata`);
	} catch (_e) {
		// The second question is a courtesy; failing it does not change the first
		// answer.
		return cause;
	}

	const { metadata, viewer_id: viewer } = GateMetadata.parse(answer.body);

	if (!metadata || metadata.is_gate_open) return cause;

	return new Error(closedGate(post, viewer));
}

// Asks for the file with whatever session the browser has. ToneDen's answer
// shape is not documented and has moved around, so take the URL from wherever
// it turns up — including a plain redirect straight at the file.
async function fetchDownload(post: Post): Promise<string> {
	let answer;

	try {
		answer = await api(`/api/v1/attachments/${post.id}/download`);
	} catch (error) {
		throw await whyRefused(post, error);
	}

	const { response, body } = answer;
	const url = DownloadAnswer.parse(body) || (response.redirected ? response.url : '');

	// A yes with nothing in it is a no by another route, and gets the same
	// second question.
	if (!url) throw await whyRefused(post, new Error(SIGN_IN));

	return url;
}

const titleOf = (post: Post) =>
	[(post.author && post.author.display_name) || '', post.title || ''].filter(Boolean).join(' — ');

// A gate the artist pointed somewhere else. The destination is in the post, so
// there is nothing to unlock.
function redirected(post: Post): Unlocked | null {
	const url = post.direct_url || post.download_display_url || '';

	if (!url || !/^https?:/i.test(url)) return null;
	const title = titleOf(post);

	return looksLikeFile(url)
		? { kind: 'file', url, title }
		: {
				kind: 'link',
				url,
				title,
				why: 'this gate is a redirect: the artist pointed it at a page, not a file.'
			};
}

async function unlockPost(post: Post): Promise<Unlocked> {
	if (post.download_source === 'redirect') {
		const answer = redirected(post);

		if (answer) return answer;
	}

	if (post.unlock_type && post.unlock_type !== 'download') {
		throw new Error(`that’s a "${post.unlock_type}" gate, not a download — there’s no file behind it.`);
	}

	// The artist turned the file off and kept the gate: the follow is the whole
	// point of the post, and there is nothing to save at the end of it.
	if (post.skip_download) {
		throw new Error('this gate has no file behind it — the artist asks for the follow only.');
	}

	const url = await fetchDownload(post);
	const title = titleOf(post);

	const filename =
		post.download_display_url && !/^https?:/i.test(post.download_display_url) ? post.download_display_url : '';

	return looksLikeFile(url) || filename
		? { kind: 'file', url, title, filename }
		: {
				kind: 'link',
				url,
				title,
				why: 'ToneDen answered with a page rather than a file.'
			};
}

// Annotated as a Site, not just checked against one: its `unlock` ignores the
// details every site is handed, and callers still hand them over.
export const toneden: Site = {
	id: 'toneden',
	name: 'ToneDen',
	hosts: ['toneden.io'],
	subfolder: 'ToneDen',
	// Only the redirect gates and the already-connected ones end in a file.
	yields: 'mixed',
	wantsEmail: false,
	async unlock(pageUrl) {
		return unlockPost(await lookupPost(pageUrl));
	}
};

// --- fanlink.tv, and fanlink.to before it ------------------------------------

/**
 * The address a FanLink is actually read from.
 *
 * fanlink.to was ToneDen's earlier domain for these links, and it is gone:
 * every path on it now answers "Brand not found". The paths came across
 * unchanged, though, so a dead .to link is a working .tv one with a different
 * suffix — which is what makes this a rewrite rather than a guess.
 *
 * Nothing ever fetches fanlink.to: the URL is rewritten before the request, so
 * the manifest names only fanlink.tv (see `aliases` below).
 */
export function fanLinkUrl(url: string) {
	const parsed = new URL(url);
	parsed.protocol = 'https:';
	parsed.hostname = 'fanlink.tv';

	return parsed.href;
}

/** The link as the page carries it, with its services and its destination. */
export function parseFanLink(html: string): FanLink | null {
	const link = FanLink.safeParse(jsonAfter(html, 'window.preloadLink =') ?? jsonAfter(html, 'window.preloadLink='));

	return link.success && link.data.id ? link.data : null;
}

// A FanLink names its author separately from its title, but not reliably: some
// carry the artist ("H V D E S"), and some carry the page's whole description
// ("Listen and download <title> by <artist> for free on ToneDen"), which has
// the title inside it already. When either string contains the other, joining
// them only says the same thing twice, so the title alone is the answer.
export function fanLinkTitle(link: FanLink) {
	const title = (link.title || '').trim();
	const author = (link.author || '').trim();

	if (!title || !author) return title || author;

	const overlaps =
		author.toLowerCase().includes(title.toLowerCase()) || title.toLowerCase().includes(author.toLowerCase());

	return overlaps ? title : `${author} — ${title}`;
}

/** Whether a URL is a ToneDen post link — a gate, in other words. */
const isPostLink = (url: string) => {
	try {
		return new URL(url).hostname.replace(/^www\./, '') === 'toneden.io' && postPathOf(url) !== null;
	} catch (_e) {
		return false;
	}
};

async function unlockFanLink(rawUrl: string): Promise<Unlocked> {
	const pageUrl = fanLinkUrl(rawUrl);

	const page = await fetch(pageUrl, {
		credentials: 'include',
		headers: { Accept: 'text/html' }
	});

	if (!page.ok) throw new Error(`The link page returned HTTP ${page.status}.`);

	const link = parseFanLink(await page.text());

	if (!link) throw new Error('that’s not a ToneDen FanLink.');

	const title = fanLinkTitle(link);

	// A FanLink can have a download gate hung off it, which is a ToneDen unlock
	// like any other.
	const attachment = link.rsvp_attachment;

	if (attachment && attachment.id) return { title, ...(await unlockPost(attachment)) };

	// Some FanLinks — most of the ones that came over from fanlink.to — are a
	// gate wearing a smart link's clothes: no services at all, and a destination
	// that is a ToneDen post. Following that is the difference between offering a
	// page to open and finishing the job.
	const target = /^https?:/i.test(link.target_url || '') ? link.target_url! : '';

	if (isPostLink(target)) return { title, ...(await unlockPost(await lookupPost(target))) };

	const services = (link.services || []).filter((service) => service.url);

	const best =
		services.find((service) => looksLikeFile(service.url!)) ??
		services.find((service) => /bandcamp|beatport|itunes/i.test(service.service_name || ''));

	// `target_url` is not always a web address — a "music" link can carry a
	// `spotify:album:…` URI, which is for the app and not for a tab.
	const url = (best && best.url) || target || pageUrl;
	const names = services.map((service) => service.service_name).filter(Boolean);

	if (looksLikeFile(url)) return { kind: 'file', url, title };

	return {
		kind: 'link',
		url,
		title,
		why: names.length
			? `a smart link, not a gate — it points at ${names.join(', ')}.`
			: 'a smart link, not a gate: there’s no file behind it.'
	};
}

export const fanlink: Site = {
	id: 'fanlink',
	name: 'FanLink',
	hosts: ['fanlink.tv'],
	// Claimed, but never fetched: fanLinkUrl() rewrites a .to link to .tv before
	// the request, so this host needs no `host_permissions` entry of its own.
	aliases: ['fanlink.to'],
	subfolder: 'ToneDen',
	yields: 'mixed',
	wantsEmail: false,
	unlock: unlockFanLink
};
