// src/tools/third-party-downloads/sites/index.ts — the registry, and matching
// a link to the site that claims it.
import { describe, it, expect } from 'vitest';
import {
	CLAIMED,
	extractGateUrls,
	HOSTS,
	isGateUrl,
	labelOfUrl,
	SITES,
	siteFor,
	unwrapGate
} from '../../src/tools/third-party-downloads/sites/index.ts';

describe('the registry', () => {
	it('describes every site the same way', () => {
		for (const site of SITES) {
			expect(site.id, site.id).toMatch(/^[a-z]+$/);
			expect(site.name).toBeTruthy();
			expect(site.subfolder).toMatch(/^[A-Za-z]+$/);
			expect(site.hosts.length).toBeGreaterThan(0);
			expect(['file', 'mixed']).toContain(site.yields);
			expect(site.wantsEmail).toBeTypeOf('boolean');
			expect(site.unlock).toBeTypeOf('function');
		}

		expect(new Set(SITES.map((site) => site.id)).size).toBe(SITES.length);
		expect(new Set(CLAIMED).size).toBe(CLAIMED.length);
	});

	it('covers the seven sites the tool is for', () => {
		expect(HOSTS).toEqual([
			'hypeddit.com',
			'toneden.io',
			'droploud.com',
			'pumpyoursound.com',
			'laylo.com',
			'ffm.to',
			'fanlink.tv'
		]);
		// fanlink.to is claimed but never fetched: it is rewritten to fanlink.tv.
		expect(CLAIMED).toEqual([...HOSTS, 'fanlink.to']);
	});
});

describe('unwrapGate', () => {
	it('unwraps SoundCloud’s redirector, and leaves everything else alone', () => {
		expect(unwrapGate('https://gate.sc?url=https%3A%2F%2Fhypeddit.com%2Fx&token=t')).toBe('https://hypeddit.com/x');
		expect(unwrapGate('https://gate.sc?token=t')).toBe('https://gate.sc?token=t');
		expect(unwrapGate('https://laylo.com/a/b')).toBe('https://laylo.com/a/b');
		expect(unwrapGate('not a url')).toBe('not a url');
	});
});

describe('siteFor', () => {
	it('finds the site a link belongs to, past www and case', () => {
		expect(siteFor('https://www.ToneDen.io/artist/post/one')!.id).toBe('toneden');
		expect(siteFor('https://droploud.com/gate/abc')!.name).toBe('Droploud');
		expect(siteFor('http://ffm.to/x')!.id).toBe('featurefm');
		expect(siteFor('https://fanlink.tv/x')!.subfolder).toBe('ToneDen');
		// ToneDen's old domain still arrives on SoundCloud links written years ago.
		expect(siteFor('https://fanlink.to/x')!.id).toBe('fanlink');
	});

	it('claims nothing it does not know', () => {
		expect(siteFor('https://bandcamp.com/x')).toBe(null);
		// A subdomain is somebody else's site, not the one it sits under.
		expect(siteFor('https://kannibalen.fanlink.tv/x')).toBe(null);
		expect(siteFor('ftp://hypeddit.com/x')).toBe(null);
		expect(siteFor('not a url')).toBe(null);
		expect(isGateUrl('https://hypeddit.com/x')).toBe(true);
		expect(isGateUrl('')).toBe(false);
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

	it('is empty for text with nothing in it', () => {
		expect(extractGateUrls('no links at all')).toEqual([]);
	});
});

describe('labelOfUrl', () => {
	it('uses the last path segment as a stand-in title', () => {
		expect(labelOfUrl('https://hypeddit.com/artist/track-name')).toBe('track-name');
		expect(labelOfUrl('https://laylo.com/artist/slug/')).toBe('slug');
		expect(labelOfUrl('https://hypeddit.com/')).toBe('https://hypeddit.com/');
		expect(labelOfUrl('nonsense')).toBe('nonsense');
	});
});
