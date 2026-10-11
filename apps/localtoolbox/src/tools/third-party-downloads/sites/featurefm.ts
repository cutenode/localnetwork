// Feature.fm — ffm.to
//
// Another smart link rather than a gate: an ffm.to page is a list of the places
// a release can be played or bought, and the "download" on it is usually
// iTunes or Beatport rather than a file. There is nothing to unlock, so what
// this does is read the destinations and hand back the one worth opening.
//
// Reading them without going through feature.fm's counter
// ------------------------------------------------------
// Every button on the page points at api.ffm.to/sl/e/c/<slug>?cd=<payload> —
// a click tracker that counts the click and then redirects. The payload is
// base64url JSON, and the real destination (`destUrl`) is inside it, along with
// which service it is (`srvc`). So the page tells the tool where each button
// goes without a single request to the tracker, which is both faster and one
// fewer fake click in somebody's dashboard.

import { fallback, z } from '$lib/zod';
import { decodeBase64Json } from '../parse.ts';
import { looksLikeFile } from '../save.ts';
import type { Site, UnlockDetails, Unlocked } from './index.ts';

/** A click payload, as far as it is read here: '' for what it does not say. */
const Click = fallback(z.object({ destUrl: fallback(z.string(), ''), srvc: fallback(z.string(), '') }), {
	destUrl: '',
	srvc: ''
});

/** Every service on the page, in page order, without repeats. */
export function parseServices(html: string) {
	const services = [];
	const seen = new Set<string>();

	for (const [, payload] of html.matchAll(/api\.ffm\.to\/sl\/e\/c\/[^"'\\\s]*?cd=([\w-]+)/g)) {
		const { destUrl: url, srvc: service } = Click.parse(decodeBase64Json(payload));

		if (!url || !/^https?:/i.test(url) || seen.has(url)) continue;
		seen.add(url);
		services.push({ service, url });
	}

	return services;
}

/** The title the page carries, for a row that has no track behind it. */
export function parseTitle(html: string) {
	const found = /<meta[^>]+property=["']og:title["'][^>]+content=["']([^"']*)["']/i.exec(html);

	return found ? found[1] : '';
}

// Which destination is the one to offer. A file beats everything; after that,
// the places a track is actually bought, in the order a DJ would try them.
const BUYABLE = ['bandcamp', 'beatport', 'itunes', 'amazoncdvinyl', 'amazon'];

async function unlock(pageUrl: string, _details: UnlockDetails): Promise<Unlocked> {
	// Some of these links are written down as http://; the manifest reaches
	// feature.fm over https, and so does the site.
	const url = new URL(pageUrl);
	url.protocol = 'https:';

	const page = await fetch(url.href, { headers: { Accept: 'text/html' } });

	if (!page.ok) throw new Error(`the link page returned HTTP ${page.status}.`);

	const html = await page.text();
	const services = parseServices(html);
	const title = parseTitle(html);

	if (services.length === 0) throw new Error('nothing on that feature.fm page to point at.');

	const file = services.find((service) => looksLikeFile(service.url));

	if (file) return { kind: 'file', url: file.url, title };

	const buy = BUYABLE.map((name) => services.find((s) => s.service === name)).find(Boolean);
	const names = services.map((service) => service.service).filter(Boolean);

	return {
		kind: 'link',
		url: (buy && buy.url) || url.href,
		title,
		why: `a smart link, not a gate — it points at ${names.join(', ')}.`
	};
}

export default {
	id: 'featurefm',
	name: 'Feature.fm',
	hosts: ['ffm.to'],
	subfolder: 'FeatureFM',
	yields: 'mixed',
	wantsEmail: false,
	unlock
} satisfies Site;
