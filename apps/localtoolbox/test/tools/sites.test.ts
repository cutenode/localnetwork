// src/tools/third-party-downloads/sites/index.ts: the registry, and matching
// a link to the site that claims it.
import { describe, it, expect, vi } from 'vitest';
import {
	CLAIMED,
	extractGateUrls,
	hostOf,
	isGateUrl,
	labelOfUrl,
	openUrlOf,
	SITES,
	siteFor,
	unwrapGate,
	type Site
} from '../../src/tools/third-party-downloads/sites/index.ts';

/** The sites of one kind, typed as that kind. */
const sitesThat = <How extends Site['how']>(how: How) =>
	SITES.filter((site): site is Extract<Site, { how: How }> => site.how === how);

describe('the registry', () => {
	it('describes every site the same way', () => {
		for (const site of SITES) {
			expect(site.id, site.id).toMatch(/^[a-z]+$/);
			expect(site.name).toBeTruthy();
			expect(site.hosts.length).toBeGreaterThan(0);
			expect(['walk', 'page', 'link']).toContain(site.how);
		}

		for (const site of sitesThat('walk')) {
			expect(site.subfolder).toMatch(/^[A-Za-z]+$/);
			// Its walk is in walks/: gates.test.ts walks it.
		}

		for (const site of sitesThat('page')) {
			expect(site.subfolder).toMatch(/^[A-Za-z]+$/);
			expect(site.page.say).toBeTruthy();
			// Each selector is one the page can be asked about.
			const selectors = [site.page.steps, site.page.email ?? '', site.page.signup ?? ''].filter(Boolean);

			for (const selector of selectors) expect(() => document.querySelector(selector), selector).not.toThrow();
		}

		for (const site of sitesThat('link')) expect(site.note).toBeTruthy();

		expect(new Set(SITES.map((site) => site.id)).size).toBe(SITES.length);
		expect(new Set(CLAIMED).size).toBe(CLAIMED.length);
	});

	it('covers the seven sites the tool is for, and fetches from one', async () => {
		// Loaded afresh, so the lists are built inside the test, where a mutation
		// run can reach the code that builds them.
		vi.resetModules();
		const fresh = await import('../../src/tools/third-party-downloads/sites/index.ts');

		// Only the walk fetches anything.
		expect(fresh.HOSTS).toEqual(['hypeddit.com']);
		// The ones done on their own page, where gate-page.ts runs.
		expect(fresh.PAGE_HOSTS).toEqual(['droploud.com', 'pumpyoursound.com', 'laylo.com']);
		// fanlink.to is claimed and rewritten to fanlink.tv when opened.
		expect(fresh.CLAIMED).toEqual([
			'hypeddit.com',
			'toneden.io',
			'droploud.com',
			'pumpyoursound.com',
			'laylo.com',
			'ffm.to',
			'fanlink.tv',
			'fanlink.to'
		]);
	});
});

describe('unwrapGate', () => {
	it('unwraps SoundCloud’s redirector, and leaves everything else alone', () => {
		expect(unwrapGate('https://gate.sc?url=https%3A%2F%2Fhypeddit.com%2Fx&token=t')).toBe('https://hypeddit.com/x');
		expect(unwrapGate('https://gate.sc?token=t')).toBe('https://gate.sc?token=t');
		expect(unwrapGate('https://laylo.com/a/b')).toBe('https://laylo.com/a/b');
		expect(unwrapGate('not a url')).toBe('not a url');
		// A `url` parameter on any other site is that site's business.
		expect(unwrapGate('https://hypeddit.com/x?url=https%3A%2F%2Fdroploud.com%2Fy')).toBe(
			'https://hypeddit.com/x?url=https%3A%2F%2Fdroploud.com%2Fy'
		);
	});
});

describe('siteFor', () => {
	it('finds the site a link belongs to, past www and case', () => {
		expect(siteFor('https://www.ToneDen.io/artist/post/one')!.id).toBe('toneden');
		expect(siteFor('https://droploud.com/gate/abc')!.name).toBe('Droploud');
		expect(siteFor('http://ffm.to/x')!.id).toBe('featurefm');
		expect(siteFor('https://fanlink.tv/x')!.how).toBe('link');
		// ToneDen's old domain still arrives on SoundCloud links written years ago.
		expect(siteFor('https://fanlink.to/x')!.id).toBe('fanlink');
	});

	it('claims nothing it does not know', () => {
		expect(siteFor('https://bandcamp.com/x')).toBe(null);
		// A subdomain is somebody else's site, not the one it sits under.
		expect(siteFor('https://kannibalen.fanlink.tv/x')).toBe(null);
		// Only a leading www. is dropped: one further in is part of somebody else's name.
		expect(siteFor('https://hypeddit.www.com/x')).toBe(null);
		expect(siteFor('ftp://hypeddit.com/x')).toBe(null);
		expect(siteFor('not a url')).toBe(null);
		expect(isGateUrl('https://hypeddit.com/x')).toBe(true);
		expect(isGateUrl('')).toBe(false);
	});
});

describe('hostOf', () => {
	it('is the lowercased host without www, for web links only', () => {
		expect(hostOf('https://www.Droploud.com/gate/a')).toBe('droploud.com');
		expect(hostOf('http://cdn.droploud.com/f.mp3')).toBe('cdn.droploud.com');
		expect(hostOf('ftp://droploud.com/x')).toBe('');
		expect(hostOf('')).toBe('');
	});
});

describe('openUrlOf', () => {
	it('opens a link on a site’s alias on its own host, path and all', () => {
		expect(openUrlOf('http://fanlink.to/abc?x=1')).toBe('https://fanlink.tv/abc?x=1');
		expect(openUrlOf('https://www.fanlink.to/abc')).toBe('https://fanlink.tv/abc');
	});

	it('leaves every other link as it is', () => {
		expect(openUrlOf('https://fanlink.tv/abc')).toBe('https://fanlink.tv/abc');
		expect(openUrlOf('https://droploud.com/gate/a')).toBe('https://droploud.com/gate/a');
		expect(openUrlOf('https://bandcamp.com/x')).toBe('https://bandcamp.com/x');
	});
});

describe('extractGateUrls', () => {
	it('picks the links it knows out of anything paste-shaped', () => {
		const text = `
      here you go: https://hypeddit.com/886rp1, and
      (https://www.toneden.io/artist/post/one)
      https://bandcamp.com/skip-me
      https://gate.sc?url=https%3A%2F%2Fdroploud.com%2Fgate%2Fabc&token=t
    `;

		expect(extractGateUrls(text)).toEqual([
			'https://hypeddit.com/886rp1',
			'https://www.toneden.io/artist/post/one',
			'https://droploud.com/gate/abc'
		]);
	});

	it('takes plain http links too', () => {
		expect(extractGateUrls('old one: http://hypeddit.com/886rp1')).toEqual(['http://hypeddit.com/886rp1']);
	});

	it('is empty for text with nothing in it', () => {
		expect(extractGateUrls('no links at all')).toEqual([]);
	});
});

describe('labelOfUrl', () => {
	it('uses the last path segment as a stand-in title', () => {
		expect(labelOfUrl('https://hypeddit.com/artist/track-name')).toBe('track-name');
		expect(labelOfUrl('https://laylo.com/artist/slug/')).toBe('slug');
		expect(labelOfUrl('https://laylo.com/artist/slug//')).toBe('slug');
		expect(labelOfUrl('https://hypeddit.com/')).toBe('https://hypeddit.com/');
		expect(labelOfUrl('nonsense')).toBe('nonsense');
	});
});
