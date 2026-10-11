// content.ts — artist profiles (the GET_ARTIST_PROFILES message): who is
// behind a path, and the links on their profile, two requests per artist.
import { describe, it, expect, afterEach, beforeEach, vi } from 'vitest';
import { loadContentScript, disposeContentScripts } from '../helpers/load.ts';
import { mockFetch, jsonResponse } from '../helpers/api.ts';
import { flush } from '../helpers/flush.ts';
import { render, hydrationScript, apiClientEntry, CLIENT_ID } from '../helpers/dom.ts';
import type { ChromeMock, Json } from '../helpers/chrome.ts';

declare const chrome: ChromeMock;

beforeEach(() => {
	vi.useFakeTimers();
});

afterEach(disposeContentScripts);

const ask = (permalinks: string[]) => chrome.testSendMessage({ type: 'GET_ARTIST_PROFILES', permalinks });

/** The profiles a lookup found: a failed one, which no test reading them expects, fails here. */
const profilesFor = async (permalinks: string[]) => {
	const answer = await ask(permalinks);

	if (!answer.ok) throw new Error(`the lookup failed: ${answer.error}`);

	return answer;
};

// A page with the client id in it — any SoundCloud page will do.
const page = () => {
	render('<p>the stream</p>', '/stream');
	hydrationScript([apiClientEntry()]);
};

// An API user object, as /resolve answers for a profile URL.
const user = (permalink: string, extra = {}) => ({
	kind: 'user',
	id: 42,
	permalink: permalink.slice(1),
	permalink_url: `https://soundcloud.com${permalink}`,
	username: 'Bicep',
	full_name: 'Andy & Matt',
	city: 'Belfast',
	country_code: 'GB',
	followers_count: 1234,
	track_count: 42,
	verified: true,
	avatar_url: 'https://i1.sndcdn.com/avatars-abc-large.jpg',
	description: 'bookings: agent@example.com',
	...extra
});

const webProfile = (network: string, url: string, extra = {}) => ({
	kind: 'web-profile',
	network,
	title: network,
	url,
	...extra
});

const resolvedPath = (url: URL) => new URL(url.searchParams.get('url')!).pathname;

/** Answers /resolve with `users` by path and /users/<id>/web-profiles with `links`. */
const api = (users: Record<string, Json>, links: Record<string, Json[]> = {}) =>
	mockFetch((url) => {
		if (url.pathname === '/resolve') {
			const found = users[resolvedPath(url)];

			return found ? jsonResponse(found) : jsonResponse({ error: 'not found' }, 404);
		}

		const match = /^\/users\/([^/]+)\/web-profiles$/.exec(url.pathname);

		if (match) return jsonResponse(links[match[1]] ?? []);

		return undefined;
	});

describe('before asking', () => {
	it('answers nothing for no artists, or none that are artists, without a request', async () => {
		page();
		const { fn } = mockFetch();
		await loadContentScript();

		for (const permalinks of [undefined, [], 'nope', ['/a/b', 7, null, 'bicep', '']]) {
			// @ts-expect-error -- what an older or broken panel might send: no list, a string, non-paths
			expect(await profilesFor(permalinks)).toEqual({ ok: true, profiles: [], failed: [], error: '' });
		}

		expect(fn).not.toHaveBeenCalled();
	});

	it('reports no-client-id when the page carries none', async () => {
		render('<p>nothing</p>', '/stream');
		const { fn } = mockFetch();
		await loadContentScript();
		expect(await profilesFor(['/bicep'])).toEqual({ ok: true, profiles: [], failed: [], error: 'no-client-id' });
		expect(fn).not.toHaveBeenCalled();
	});
});

describe('one artist', () => {
	it('resolves the path, then asks for the links, and keeps what the panel shows', async () => {
		page();

		const { paths, calls } = api(
			{ '/bicep': user('/bicep') },
			{
				42: [
					webProfile('instagram', 'https://www.instagram.com/bicepmusic/', { title: 'IG' }),
					webProfile('Personal', ' https://feelmybicep.com ', { title: '  my   site ' })
				]
			}
		);

		await loadContentScript();
		const response = await profilesFor(['/bicep']);
		expect(response).toEqual({
			ok: true,
			error: '',
			failed: [],
			profiles: [
				{
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
					bio: 'bookings: agent@example.com',
					links: [
						{ network: 'instagram', title: 'IG', url: 'https://www.instagram.com/bicepmusic/' },
						{ network: 'personal', title: 'my site', url: 'https://feelmybicep.com' }
					]
				}
			]
		});
		expect(paths()).toEqual([
			`/resolve?url=https%3A%2F%2Fsoundcloud.com%2Fbicep&client_id=${CLIENT_ID}`,
			`/users/42/web-profiles?client_id=${CLIENT_ID}`
		]);

		// Anonymous, as every request to SoundCloud is.
		for (const call of calls) expect(call.init).toEqual({ credentials: 'omit' });
	});

	it('says nothing it was not told, and files the profile under the path it was asked for', async () => {
		page();
		api({
			'/bicep': {
				kind: 'user',
				id: 42,
				permalink_url: 'https://soundcloud.com/Bicep',
				username: 7,
				followers_count: 'many',
				track_count: 1.5,
				verified: 'yes',
				avatar_url: null,
				description: ['no']
			}
		});
		await loadContentScript();
		const [profile] = (await profilesFor(['/bicep'])).profiles;
		expect(profile).toEqual({
			id: 42,
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
			links: []
		});
	});

	it('cuts a long bio, and keeps at most twenty links that are addresses', async () => {
		page();
		const links = Array.from({ length: 25 }, (_, n) => webProfile('personal', `https://site${n}.example`));
		api(
			{ '/bicep': user('/bicep', { description: `  ${'b'.repeat(2000)}  ` }) },
			{ 42: [null, webProfile('email', 'mailto:a@b.c'), webProfile('twitter', ''), { url: 7 }, ...links] }
		);
		await loadContentScript();
		const [profile] = (await profilesFor(['/bicep'])).profiles;
		expect(profile.bio).toBe('b'.repeat(1000));
		expect(profile.links).toHaveLength(20);
		expect(profile.links[0].url).toBe('https://site0.example');
	});

	it('keeps no id that is not a whole number, and a title that is not words', async () => {
		page();
		api({ '/bicep': user('/bicep', { id: 'x' }) }, { x: [{ url: 'https://a.example', title: 7 }] });
		await loadContentScript();
		const [profile] = (await profilesFor(['/bicep'])).profiles;
		expect(profile.id).toBe(null);
		expect(profile.links).toEqual([{ network: '', title: '', url: 'https://a.example' }]);
	});

	it('files a link under `service` when there is no `network`, and under nothing when there is neither', async () => {
		page();
		api(
			{ '/bicep': user('/bicep') },
			{
				42: [{ url: 'https://x.com/a', service: 'Twitter' }, { url: 'https://y.com/a' }]
			}
		);
		await loadContentScript();
		const [profile] = (await profilesFor(['/bicep'])).profiles;
		expect(profile.links.map((link) => [link.network, link.title])).toEqual([
			['twitter', ''],
			['', '']
		]);
	});

	it('has no links when SoundCloud answers with something that is not a list of them', async () => {
		page();
		mockFetch((url) => (url.pathname === '/resolve' ? jsonResponse(user('/bicep')) : jsonResponse({ collection: [] })));
		await loadContentScript();
		expect((await profilesFor(['/bicep'])).profiles[0].links).toEqual([]);
	});

	it('is still a profile when its links cannot be fetched', async () => {
		page();
		mockFetch((url) =>
			url.pathname === '/resolve' ? jsonResponse(user('/bicep')) : jsonResponse({ error: 'x' }, 500)
		);
		await loadContentScript();
		const response = await profilesFor(['/bicep']);
		expect(response.profiles[0]).toMatchObject({ permalink: '/bicep', links: [] });
		expect(response.failed).toEqual([]);
		expect(response.error).toBe('');
	});

	it('is a failure for a path SoundCloud has no user behind', async () => {
		page();
		api({ '/a-track': { kind: 'track', id: 1 }, '/no-id': { kind: 'user' } });
		await loadContentScript();
		const response = await profilesFor(['/a-track', '/no-id', '/nobody']);
		expect(response.profiles).toEqual([]);
		expect(response.failed.sort()).toEqual(['/a-track', '/no-id', '/nobody']);
		expect(response.error).toBe('');
	});
});

describe('several artists', () => {
	it('asks four at a time, answers each once, and reports progress as it goes', async () => {
		page();
		const users = Object.fromEntries(Array.from({ length: 6 }, (_, n) => [`/a${n}`, user(`/a${n}`, { id: n + 1 })]));
		let inFlight = 0;
		let most = 0;
		mockFetch(async (url) => {
			inFlight += 1;
			most = Math.max(most, inFlight);
			await new Promise((resolve) => setImmediate(resolve));
			inFlight -= 1;

			if (url.pathname === '/resolve') return jsonResponse(users[resolvedPath(url)]);

			return jsonResponse([]);
		});
		await loadContentScript();
		chrome.runtime.sendMessage.mockClear();

		const response = await profilesFor(['/a0', '/a1', '/a2', '/a3', '/a4', '/a5', '/a0']);
		expect(response.profiles.map((p) => p.permalink).sort()).toEqual(['/a0', '/a1', '/a2', '/a3', '/a4', '/a5']);
		expect(most).toBeLessThanOrEqual(4);
		expect(most).toBeGreaterThan(1);

		const progress = chrome.runtime.sendMessage.mock.calls
			.map(([message]) => message)
			.filter((message) => message.type === 'ARTIST_PROFILES_PROGRESS');

		expect(progress).toHaveLength(6);
		expect(progress.map((message) => message.done).sort((a, b) => a - b)).toEqual([1, 2, 3, 4, 5, 6]);
		expect(progress.every((message) => message.total === 6)).toBe(true);
		expect(
			progress
				.flatMap((message) => message.profiles)
				.map((p) => p.permalink)
				.sort()
		).toEqual(['/a0', '/a1', '/a2', '/a3', '/a4', '/a5']);
		expect(progress.every((message) => message.failed.length === 0)).toBe(true);
	});

	it('reports a failure as one too', async () => {
		page();
		api({});
		await loadContentScript();
		chrome.runtime.sendMessage.mockClear();
		await ask(['/nobody']);
		expect(chrome.runtime.sendMessage).toHaveBeenCalledWith({
			type: 'ARTIST_PROFILES_PROGRESS',
			done: 1,
			total: 1,
			profiles: [],
			failed: ['/nobody']
		});
	});

	it('resolves a path once per page, however often it is asked about', async () => {
		page();
		const { paths } = api({ '/bicep': user('/bicep') });
		await loadContentScript();
		await ask(['/bicep']);
		await ask(['/bicep']);
		expect(paths().filter((p) => p.startsWith('/resolve'))).toHaveLength(1);
		expect(paths().filter((p) => p.includes('web-profiles'))).toHaveLength(2);
	});

	it('asks again about a path whose resolve failed', async () => {
		page();
		let attempts = 0;
		mockFetch((url) => {
			if (url.pathname === '/resolve') {
				attempts += 1;

				return attempts === 1 ? jsonResponse({ error: 'x' }, 500) : jsonResponse(user('/bicep'));
			}

			return jsonResponse([]);
		});
		await loadContentScript();
		expect((await profilesFor(['/bicep'])).failed).toEqual(['/bicep']);
		expect((await profilesFor(['/bicep'])).profiles).toHaveLength(1);
	});
});

describe('when SoundCloud rejects the client id', () => {
	it('stops at a 401 on a resolve, and says so', async () => {
		page();
		const { fn } = mockFetch(() => jsonResponse({ error: 'unauthorized' }, 401));
		await loadContentScript();
		const response = await profilesFor(['/a', '/b', '/c', '/d', '/e', '/f', '/g', '/h']);
		expect(response).toEqual({ ok: true, profiles: [], failed: [], error: 'unauthorized' });
		// The four in the air, and not the rest.
		expect(fn.mock.calls.length).toBeLessThanOrEqual(4);
	});

	it('stops at a 401 on the links too', async () => {
		page();
		mockFetch((url) =>
			url.pathname === '/resolve' ? jsonResponse(user('/bicep')) : jsonResponse({ error: 'unauthorized' }, 401)
		);
		await loadContentScript();
		const response = await profilesFor(['/bicep']);
		expect(response).toEqual({ ok: true, profiles: [], failed: [], error: 'unauthorized' });
	});

	it('backs off and retries a 429 before giving up', async () => {
		page();
		let n = 0;
		mockFetch((url) => {
			if (url.pathname !== '/resolve') return jsonResponse([]);
			n += 1;

			return n < 3 ? jsonResponse({ error: 'slow down' }, 429) : jsonResponse(user('/bicep'));
		});
		await loadContentScript();
		const asked = profilesFor(['/bicep']);
		await flush();
		await vi.advanceTimersByTimeAsync(1500);
		await flush();
		await vi.advanceTimersByTimeAsync(3000);
		expect((await asked).profiles).toHaveLength(1);
	});
});

describe('when the runtime itself fails', () => {
	it('answers with the error rather than leaving the panel waiting', async () => {
		page();
		api({ '/bicep': user('/bicep') });
		await loadContentScript();
		chrome.runtime.sendMessage.mockImplementation(() => {
			throw new Error('Extension context invalidated');
		});
		expect(await ask(['/bicep'])).toEqual({ ok: false, error: 'Extension context invalidated' });

		// Whatever was thrown, in its own words.
		chrome.runtime.sendMessage.mockImplementation(() => {
			throw 'gone';
		});
		expect(await ask(['/other'])).toEqual({ ok: false, error: 'gone' });
	});
});
