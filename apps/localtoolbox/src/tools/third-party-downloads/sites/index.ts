// The sites this tool knows, and how a link is matched to one.
//
// Each site is one module under this folder, exporting the same shape:
//
//   id          what the panel calls it internally
//   name        what a row's badge says
//   hosts       the hostnames it claims, without "www."
//   aliases     hostnames it claims too, rewritten to its first host before a
//               link is opened (a site that has moved domain). Optional.
//   how         what Get does with one of its links:
//
//     'walk'    the panel fetches the gate and posts back what the page would,
//               and saves the file: its walk, in ../walks/, is
//               unlock(url, {email, name, connect}). Its hosts are the
//               extension's `host_permissions`.
//     'page'    Get opens the gate in a tab and the person does it there.
//               gate-page.ts, a content script on its hosts, highlights what
//               to press and fills the email field (`page`, below), and the
//               background worker files the download the page starts into
//               `subfolder` (./on-page.ts). Nothing is fetched or posted.
//     'link'    nothing to do but open it: the row is an Open button, with
//               `note` under it.
//
// `connect(url, {landing})` is the one thing a walk cannot do with fetch: it
// sends a tab out through a sign-in and answers whether it came back to
// `landing`. An OAuth step is a chain of redirects through somebody else's
// login, so only a tab can walk it, and that way only the gate's own server
// learns anything from it. It answers false rather than throwing when the trip
// needs a person (a login, a consent screen), leaving the walk to say so in its
// own words. It is absent outside the panel.
//
// `unlock` either throws an Error whose message is what the row should say, or
// answers {url, note?}: the file to hand the downloader, and what the row
// says about it.
//
// A thrown error sends the person to the gate itself, which is what a failed
// row offers. A gate that failed for want of something the panel can ask for
// instead says so by throwing a `Wanting` from ../next-step.ts.
//
// Adding a site is a module here and a line in SITES. A walked site's hosts go
// in `host_permissions` in src/manifest.json, without which every fetch it makes
// is blocked; a page site's go in the gate-page content script's `matches`.
// test/consistency.test.ts holds the manifest to both lists.

import hypeddit from './hypeddit.ts';
import droploud from './droploud.ts';
import pumpyoursound from './pumpyoursound.ts';
import laylo from './laylo.ts';
import featurefm from './featurefm.ts';
import { toneden, fanlink } from './toneden.ts';

interface Named {
	id: string;
	name: string;
	hosts: string[];
	aliases?: string[];
}

/** The sites with a walk of their own in ../walks/. */
export type WalkedId = 'hypeddit';

/** A site whose gates the panel walks itself (above). */
export interface WalkedSite extends Named {
	id: WalkedId;
	how: 'walk';
	subfolder: string;
}

/** A site whose gates the person does on its own page, with the help `page` describes (above). */
export interface PageSite extends Named {
	how: 'page';
	subfolder: string;
	page: PageHelp;
}

/** A site with nothing to do but open (above). */
export interface LinkSite extends Named {
	how: 'link';
	/** The line under the row. */
	note: string;
}

/** One of the sites, as its module exports it (above). */
export type Site = WalkedSite | PageSite | LinkSite;

/** What gate-page.ts does on a page site's gate, by CSS selector. */
export interface PageHelp {
	/** What the bar it puts on the page says. */
	say: string;
	/** What it outlines: the steps, or the button that ends them. */
	steps: string;
	/** The email field it fills from settings while that is empty, if the page has one. */
	email?: string;
	/** The form whose submit is the sign-up, on a page that ends in one rather than a file. */
	signup?: string;
}

/** What a walk's `unlock` is handed: the settings a gate may ask for, and `connect` (above). */
export interface UnlockDetails {
	/** The address in settings, or ''. */
	email: string;
	/** The name beside it, or ''. */
	name: string;
	connect?: (url: string, options: { landing: string }) => Promise<boolean>;
}

/** What a walk's `unlock` answers with, when it does not throw (above). */
export interface Unlocked {
	/** The file. */
	url: string;
	/** Something the walk did on the person's own account, for the row to say. */
	note?: string;
}

export const SITES: Site[] = [hypeddit, toneden, droploud, pumpyoursound, laylo, featurefm, fanlink];

const hostsOf = (how: Site['how']) => SITES.flatMap((site) => (site.how === how ? site.hosts : []));

/** Every hostname a walk fetches: what `host_permissions` covers. @internal */
export const HOSTS = hostsOf('walk');

/** Every hostname gate-page.ts runs on: its `matches`. @internal */
export const PAGE_HOSTS = hostsOf('page');

/** Every hostname a link can arrive on. @internal */
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

/** The hostname a link is matched on: lowercased, without "www.", or '' for no URL at all. */
export function hostOf(url: string) {
	try {
		const parsed = new URL(url);

		if (parsed.protocol !== 'https:' && parsed.protocol !== 'http:') return '';

		return parsed.hostname.replace(/^www\./, '').toLowerCase();
	} catch (_e) {
		return '';
	}
}

/** The site a link belongs to, or null for a link none of them claims. */
export function siteFor(url: string): Site | null {
	const host = hostOf(url);

	if (!host) return null;

	return SITES.find((site) => site.hosts.includes(host) || (site.aliases ?? []).includes(host)) ?? null;
}

export const isGateUrl = (url: string) => siteFor(url) !== null;

/**
 * Where a link is opened: itself, or, on one of a site's `aliases`, the same
 * path on its first host. fanlink.to, ToneDen's earlier domain for FanLinks,
 * answers "Brand not found" on every path, and the paths came across to
 * fanlink.tv unchanged, so the rewrite is not a guess.
 */
export function openUrlOf(url: string) {
	const site = siteFor(url);

	if (!site?.aliases?.includes(hostOf(url))) return url;
	const parsed = new URL(url);
	parsed.protocol = 'https:';
	parsed.hostname = site.hosts[0];

	return parsed.href;
}

/** Links out of anything paste-shaped: a list, a chat log, prose. */
export function extractGateUrls(text: string) {
	// Stryker disable next-line ArrayDeclaration: equivalent. A placeholder in place of no matches is not a link any site claims, so the filter below drops it.
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
