import { expect, it } from 'vitest';
import { render } from 'vitest-browser-svelte';
import SocialMeta from './SocialMeta.svelte';

const tag = (selector: string) => document.head.querySelector(selector)?.getAttribute('content');

const href = (selector: string) => document.head.querySelector(selector)?.getAttribute('href');

it('writes the title, the canonical address and what link previews show', async () => {
	await render(SocialMeta, {
		meta: {
			title: 'Neon Tide',
			description: 'Rolling techno',
			url: 'https://localnetwork.test/projects/neon-tide',
			type: 'profile',
			image: { url: 'https://localnetwork.test/media/did:plc:root/b', alt: "Neon Tide's picture" }
		}
	});

	await expect.poll(() => document.title).toBe('Neon Tide · Local Network');
	expect(tag('meta[name="description"]')).toBe('Rolling techno');
	expect(href('link[rel="canonical"]')).toBe('https://localnetwork.test/projects/neon-tide');
	expect(tag('meta[property="og:url"]')).toBe('https://localnetwork.test/projects/neon-tide');
	expect(tag('meta[property="og:title"]')).toBe('Neon Tide');
	expect(tag('meta[property="og:site_name"]')).toBe('Local Network');
	expect(tag('meta[property="og:type"]')).toBe('profile');
	expect(tag('meta[property="og:image"]')).toBe('https://localnetwork.test/media/did:plc:root/b');
	expect(tag('meta[property="og:image:alt"]')).toBe("Neon Tide's picture");
	expect(tag('meta[name="twitter:image:alt"]')).toBe("Neon Tide's picture");
	expect(tag('meta[name="twitter:card"]')).toBe('summary');
	expect(document.head.querySelector('meta[name="robots"]')).toBeNull();
});

it('takes its own document title, and keeps a page out of search when asked, without a picture', async () => {
	await render(SocialMeta, {
		meta: {
			title: 'Local Network',
			documentTitle: 'Local Network · Find local DJs for your show',
			description: 'Promoters find local DJs.',
			url: 'https://localnetwork.test/',
			type: 'website',
			noindex: true
		}
	});

	await expect.poll(() => document.title).toBe('Local Network · Find local DJs for your show');
	expect(tag('meta[name="robots"]')).toBe('noindex');
	expect(document.head.querySelector('meta[property="og:image"]')).toBeNull();
});

it('names the records behind the page as at-tags, one tag per record', async () => {
	const project = 'at://did:plc:root/computer.localnetwork.project/1';
	const set = 'at://did:plc:a/computer.localnetwork.dj.set/3x';

	await render(SocialMeta, {
		meta: {
			title: 'Warehouse by Neon Tide',
			description: 'A tracklist',
			url: 'https://localnetwork.test/projects/neon-tide/mixes/1',
			type: 'music.song',
			records: { canonical: [project], alternate: [set], author: ['at://did:plc:a', 'at://did:plc:b'] }
		}
	});

	const values = (name: string) =>
		[...document.head.querySelectorAll(`meta[name="${name}"]`)].map((meta) => meta.getAttribute('content'));

	await expect.poll(() => values('at:canonical')).toEqual([project]);
	expect(values('at:alternate')).toEqual([set]);
	expect(values('at:author')).toEqual(['at://did:plc:a', 'at://did:plc:b']);
	expect(document.head.querySelector('meta[property^="at:"], link[href^="at:"]')).toBeNull();
});
