// The sites this tool knows how to walk, and how a link is matched to one.
//
// Each site is one module under this folder, exporting the same shape:
//
//   id          what the panel calls it internally
//   name        what a row's badge says
//   hosts       the hostnames it claims and fetches from, without "www."
//   aliases     hostnames it claims but never fetches, because it rewrites them
//               to one of its `hosts` first (a site that has moved domain).
//               Optional, and deliberately outside `host_permissions`.
//   subfolder   where its files land under the browser's download directory
//   yields      'file'  — a gate that unlocks always ends in a file
//               'mixed' — it might, or it might be a page to open instead
//   wantsEmail  whether the address in settings is ever used
//   unlock(url, {email, name, connect}) → Promise<Unlocked>
//
// `connect(url, {landing})` is the one thing an adapter cannot do with fetch:
// it sends a tab out through a sign-in and answers whether it came back to
// `landing`. An OAuth step is a chain of redirects through somebody else's
// login, so only a tab can walk it — and only the gate's own server learns
// anything from it, which is the point. It answers false rather than throwing
// when the trip needs a person (a login, a consent screen), leaving the
// adapter to say so in its own words. It is absent outside the panel.
//
// `unlock` either throws an Error whose message is what the row should say, or
// answers with one of:
//
//   {kind: 'file', url, title?, filename?}  hand it to the downloader
//   {kind: 'link', url, title?, why}        not a file: somewhere to open
//   {kind: 'signup', url, title?, why}      the address was left with the
//                                           artist, who sends the link out
//
// A thrown error sends the person to the gate itself, which is what a failed
// row offers. A gate that failed for want of something the panel can ask for
// instead says so by throwing a `Wanting` from ../next-step.ts.
//
// Adding a site is a module here and a line in SITES — plus its hosts in
// `host_permissions` in src/manifest.json, without which every fetch it makes
// is blocked.

import hypeddit from './hypeddit.ts';
import droploud from './droploud.ts';
import pumpyoursound from './pumpyoursound.ts';
import laylo from './laylo.ts';
import featurefm from './featurefm.ts';
import { toneden, fanlink } from './toneden.ts';

/** One of the sites, as its module exports it (above). */
export interface Site {
	id: string;
	name: string;
	hosts: string[];
	aliases?: string[];
	subfolder: string;
	yields: 'file' | 'mixed';
	wantsEmail: boolean;
	unlock: (pageUrl: string, details: UnlockDetails) => Promise<Unlocked>;
}

/** What `unlock` is handed: the settings a gate may ask for, and `connect` (above). */
export interface UnlockDetails {
	/** The address in settings, or ''. */
	email: string;
	/** The name beside it, or ''. */
	name: string;
	connect?: (url: string, options: { landing: string }) => Promise<boolean>;
}

/** What `unlock` answers with, when it does not throw (above). */
export type Unlocked =
	| { kind: 'file'; url: string; title?: string; filename?: string }
	| { kind: 'link' | 'signup'; url: string; title?: string; why: string };

export const SITES: Site[] = [hypeddit, toneden, droploud, pumpyoursound, laylo, featurefm, fanlink];

/** Every hostname a site actually fetches — what `host_permissions` covers. */
export const HOSTS = SITES.flatMap((site) => site.hosts);

/** Every hostname a link can arrive on, fetched or rewritten. */
export const CLAIMED = SITES.flatMap((site) => [...site.hosts, ...(site.aliases ?? [])]);

// SoundCloud wraps outbound links in its gate.sc redirector
// (https://gate.sc?url=<encoded>&token=…). The API returns the raw purchase
// URL, but unwrap defensively so a wrapped link is still recognised.
export function unwrapGate(url: string) {
	try {
		const parsed = new URL(url);

		if (parsed.hostname === 'gate.sc') {
			const inner = parsed.searchParams.get('url');

			if (inner) return inner;
		}
	} catch (_e) {
		// Not a URL; returned as is below.
	}

	return url;
}

/** The site a link belongs to, or null for a link none of them claims. */
export function siteFor(url: string): Site | null {
	let host;

	try {
		const parsed = new URL(url);

		if (parsed.protocol !== 'https:' && parsed.protocol !== 'http:') return null;
		host = parsed.hostname.replace(/^www\./, '').toLowerCase();
	} catch (_e) {
		return null;
	}

	return SITES.find((site) => site.hosts.includes(host) || (site.aliases ?? []).includes(host)) ?? null;
}

export const isGateUrl = (url: string) => siteFor(url) !== null;

/** Links out of anything paste-shaped: a list, a chat log, prose. */
export function extractGateUrls(text: string) {
	const found = text.match(/https?:\/\/[^\s<>"']+/gi) || [];

	return (
		found
			// Trailing punctuation belongs to the sentence, not to the link.
			.map((url) => url.replace(/[)\]},.;'"]+$/, ''))
			.map(unwrapGate)
			.filter(isGateUrl)
	);
}

/** The last path segment, as a stand-in title until the site names the file. */
export function labelOfUrl(url: string) {
	try {
		const path = new URL(url).pathname.replace(/\/+$/, '');

		return path.slice(path.lastIndexOf('/') + 1) || url;
	} catch (_e) {
		return url;
	}
}
