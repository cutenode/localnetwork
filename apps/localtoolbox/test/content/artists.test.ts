// content.ts, artist profiles (the GET_ARTIST_PROFILES message): who is behind
// a path, and the links on their profile, as the page loaded them. A profile
// page loads the whole account and its links; every track names its uploader
// in short, and an artist known only that way is answered in part. Nothing is
// asked of SoundCloud.
import { describe, it, expect, afterEach, beforeEach, vi } from 'vitest';
import { loadContentScript, disposeContentScripts } from '../helpers/load.ts';
import { mockFetch } from '../helpers/api.ts';
import { render, hydrationScript, apiTrack, apiUser, pageLoaded } from '../helpers/dom.ts';
import type { ChromeMock, Json } from '../helpers/chrome.ts';

declare const chrome: ChromeMock;

beforeEach(() => {
	vi.useFakeTimers();
});

afterEach(disposeContentScripts);

const ask = (permalinks: string[]) => chrome.testSendMessage({ type: 'GET_ARTIST_PROFILES', permalinks });

const profilesFor = async (permalinks: string[]) => (await ask(permalinks)).profiles;

// Any SoundCloud page will do: the answer is about what the page loaded, not
// what it shows.
const page = async (served: Json[] = []) => {
	render('<p>the stream</p>', '/stream');

	if (served.length > 0) hydrationScript(served);
	await loadContentScript();
};

// The whole account, as a profile page loads it.
const bicep = (extra: Record<string, Json> = {}) =>
	apiUser({
		id: 42,
		permalink: 'bicep',
		username: 'Bicep',
		bio: 'two lads from belfast',
		full_name: 'Andy & Matt',
		city: 'Belfast',
		country_code: 'GB',
		followers_count: 1234,
		track_count: 42,
		verified: true,
		avatar_url: 'https://i1.sndcdn.com/avatars-abc-large.jpg',
		...extra
	});

// The short form of the same account, as a track carries its uploader.
const shortBicep = () =>
	apiUser({ id: 42, permalink: 'bicep', username: 'Bicep', avatar_url: 'https://short.example/a.jpg' });

const webProfile = (network: string, url: string, extra: Record<string, Json> = {}) => ({
	kind: 'web-profile',
	network,
	title: network,
	url,
	...extra
});

const WHOLE = {
	id: 42,
	permalink: '/bicep',
	username: 'Bicep',
	fullName: 'Andy & Matt',
	city: 'Belfast',
	country: 'GB',
	followers: 1234,
	trackCount: 42,
	verified: true,
	avatarUrl: 'https://i1.sndcdn.com/avatars-abc-large.jpg',
	bio: 'two lads from belfast'
};

describe('a whole profile', () => {
	it('answers a profile page’s account and its links in full', async () => {
		await page([{ hydratable: 'user', data: bicep() }]);
		pageLoaded('/users/soundcloud:users:42/web-profiles', [
			webProfile('instagram', 'https://www.instagram.com/bicepmusic/', { title: 'IG' }),
			webProfile('Personal', ' https://feelmybicep.com ', { title: '  my   site ' })
		]);

		expect(await ask(['/bicep'])).toEqual({
			ok: true,
			profiles: [
				{
					...WHOLE,
					links: [
						{ network: 'instagram', title: 'IG', url: 'https://www.instagram.com/bicepmusic/' },
						{ network: 'personal', title: 'my site', url: 'https://feelmybicep.com' }
					]
				}
			]
		});
	});

	it.each([
		['a user by id', '/users/42'],
		['a resolved profile URL', '/resolve']
	])('learns the account from %s, and the links by bare id', async (_name, path) => {
		await page();
		pageLoaded(path, bicep());
		pageLoaded('/users/42/web-profiles', [webProfile('twitter', 'https://x.com/bicep')]);

		expect(await profilesFor(['/bicep'])).toEqual([
			{ ...WHOLE, links: [{ network: 'twitter', title: 'twitter', url: 'https://x.com/bicep' }] }
		]);
	});

	it('cuts a long bio, and keeps at most twenty links that are web addresses', async () => {
		await page([{ hydratable: 'user', data: bicep({ description: `  ${'b'.repeat(2000)}  ` }) }]);
		const links = Array.from({ length: 25 }, (_, n) => webProfile('personal', `https://site${n}.example`));
		pageLoaded('/users/42/web-profiles', [
			null,
			webProfile('email', 'mailto:a@b.c'),
			webProfile('twitter', ''),
			webProfile('ftp', 'ftp://files.example'),
			{ url: 7 },
			webProfile('plain', 'HTTP://LOUD.example'),
			...links
		]);

		const [profile] = await profilesFor(['/bicep']);
		expect(profile.bio).toBe('b'.repeat(1000));
		expect(profile.links).toHaveLength(20);
		expect(profile.links.slice(0, 2).map((link) => link.url)).toEqual(['HTTP://LOUD.example', 'https://site0.example']);
	});

	it('files a link under `service` when there is no `network`, and under nothing when there is neither', async () => {
		await page([{ hydratable: 'user', data: bicep() }]);
		pageLoaded('/users/42/web-profiles', [
			{ url: 'https://x.com/a', service: 'Twitter' },
			{ url: 'https://y.com/a', title: 7 }
		]);

		expect((await profilesFor(['/bicep']))[0].links).toEqual([
			{ network: 'twitter', title: '', url: 'https://x.com/a' },
			{ network: '', title: '', url: 'https://y.com/a' }
		]);
	});

	it('has no links where the links answer is not a list of them', async () => {
		await page([{ hydratable: 'user', data: bicep() }]);
		pageLoaded('/users/42/web-profiles', { collection: [] });

		// The links were loaded, and there are none: nothing is missing.
		expect(await profilesFor(['/bicep'])).toEqual([{ ...WHOLE, links: [] }]);
	});

	it('says nothing it was not told', async () => {
		await page([
			{
				hydratable: 'user',
				data: {
					kind: 'user',
					id: 'x',
					permalink_url: 'https://soundcloud.com/bicep',
					username: 7,
					followers_count: 'many',
					track_count: 1.5,
					verified: 'yes',
					avatar_url: null,
					description: ['no']
				}
			}
		]);

		expect(await profilesFor(['/bicep'])).toEqual([
			{
				id: null,
				permalink: '/bicep',
				username: '',
				fullName: '',
				city: '',
				country: '',
				followers: null,
				trackCount: null,
				verified: false,
				avatarUrl: '',
				bio: '',
				links: [],
				partial: true
			}
		]);
	});
});

describe('an artist known only in part', () => {
	it('answers the uploader a track carries, marked partial', async () => {
		await page();
		pageLoaded('/tracks', [{ ...apiTrack({ permalink: '/bicep/glue' }), user: shortBicep() }]);

		expect(await profilesFor(['/bicep'])).toEqual([
			{
				id: 42,
				permalink: '/bicep',
				username: 'Bicep',
				fullName: '',
				city: '',
				country: '',
				followers: null,
				trackCount: null,
				verified: false,
				avatarUrl: 'https://short.example/a.jpg',
				bio: '',
				links: [],
				partial: true
			}
		]);
	});

	it('answers a whole account whose links the page has not loaded, marked partial', async () => {
		await page([{ hydratable: 'user', data: bicep() }]);
		expect(await profilesFor(['/bicep'])).toEqual([{ ...WHOLE, links: [], partial: true }]);
	});

	it('keeps the links of an account known only in short, and stays partial', async () => {
		await page();
		pageLoaded('/resolve', { ...apiTrack({ permalink: '/bicep/glue' }), user: shortBicep() });
		pageLoaded('/users/42/web-profiles', [webProfile('twitter', 'https://x.com/bicep')]);

		const [profile] = await profilesFor(['/bicep']);
		expect(profile).toMatchObject({ partial: true, links: [{ url: 'https://x.com/bicep' }] });
	});

	it('never lets a short account replace a whole one, and lets a whole one replace a short one', async () => {
		await page([{ hydratable: 'sound', data: { ...apiTrack({ permalink: '/a/one' }), user: shortBicep() } }]);
		expect((await profilesFor(['/bicep']))[0].partial).toBe(true);

		pageLoaded('/users/42', bicep());
		pageLoaded('/users/42/web-profiles', []);
		pageLoaded('/tracks', [{ ...apiTrack({ permalink: '/bicep/two' }), user: shortBicep() }]);

		expect(await profilesFor(['/bicep'])).toEqual([{ ...WHOLE, links: [] }]);
	});
});

describe('which artists are answered', () => {
	it('answers only artist paths the page loaded, each once, and nothing for anything else', async () => {
		await page([
			{ hydratable: 'user', data: bicep() },
			// Users with no profile path of their own are not filed at all.
			{ hydratable: 'user', data: apiUser({ permalink: 'a/b' }) },
			{ hydratable: 'user', data: { kind: 'user', id: 9, username: 'nowhere' } }
		]);

		for (const permalinks of [undefined, [], 'nope', ['/a/b', 7, null, 'bicep', ''], ['/nobody']]) {
			// @ts-expect-error -- what an older or broken panel might send: no list, a string, non-paths
			expect(await ask(permalinks)).toEqual({ ok: true, profiles: [] });
		}

		const answer = await profilesFor(['/bicep', '/nobody', '/bicep']);
		expect(answer.map((profile) => profile.permalink)).toEqual(['/bicep']);
	});

	it('asks SoundCloud for nothing, and keeps nothing it learned in the extension’s storage', async () => {
		const { fn } = mockFetch();
		await page([{ hydratable: 'user', data: bicep() }]);
		await profilesFor(['/bicep', '/nobody']);
		await vi.advanceTimersByTimeAsync(60_000);

		expect(fn).not.toHaveBeenCalled();
		expect(chrome.storage.local.set).not.toHaveBeenCalled();
		expect(chrome.storage.session.set).not.toHaveBeenCalled();
	});
});
