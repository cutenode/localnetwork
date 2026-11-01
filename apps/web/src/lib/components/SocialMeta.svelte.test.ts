import { expect, it } from 'vitest';
import { render } from 'vitest-browser-svelte';
import type { PageMeta } from '#lib/meta.ts';
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

const jsonLdScripts = () => document.head.querySelectorAll('script[type="application/ld+json"]');

it('writes the structured data search engines read, and none for a page without it', async () => {
	const meta: PageMeta = {
		title: 'Neon Tide',
		description: 'Rolling techno',
		url: 'https://localnetwork.test/projects/neon-tide',
		type: 'profile'
	};

	const { rerender } = await render(SocialMeta, {
		meta: {
			...meta,
			jsonLd: {
				'@context': 'https://schema.org',
				'@type': 'ProfilePage',
				mainEntity: { '@type': 'MusicGroup', name: 'Neon Tide' }
			}
		}
	});

	await expect.poll(() => jsonLdScripts().length).toBe(1);
	expect(jsonLdScripts()[0]?.textContent).toBe(
		'{"@context":"https://schema.org","@type":"ProfilePage","mainEntity":{"@type":"MusicGroup","name":"Neon Tide"}}'
	);

	await rerender({ meta });
	await expect.poll(() => jsonLdScripts().length).toBe(0);
});

it("keeps a name that closes the script tag inside it, so it can't add a script of its own", async () => {
	const name = '</script><script>alert(1)</script>';
	const scripts = () => document.head.querySelectorAll('script');
	const before = scripts().length;

	await render(SocialMeta, {
		meta: {
			title: name,
			description: name,
			url: 'https://localnetwork.test/projects/x',
			type: 'profile',
			jsonLd: { '@context': 'https://schema.org', '@type': 'MusicGroup', name }
		}
	});

	await expect.poll(() => jsonLdScripts().length).toBe(1);
	expect(scripts().length).toBe(before + 1);
	// The name is all still there, each `<` as JSON's escape for it, which reads back as the same character.
	expect(jsonLdScripts()[0]?.textContent).toBe(
		String.raw`{"@context":"https://schema.org","@type":"MusicGroup","name":"\u003c/script>\u003cscript>alert(1)\u003c/script>"}`
	);
});
