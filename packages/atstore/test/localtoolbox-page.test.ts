import { afterEach, describe, expect, it } from 'vitest';
import { cover, installChrome, PAGE } from '../src/listings/localtoolbox-page.ts';

afterEach(() => {
	Reflect.deleteProperty(globalThis, 'chrome');
});

describe('installChrome', () => {
	it('installs itself as the global chrome', () => {
		const chrome = installChrome(PAGE);

		expect(globalThis).toHaveProperty('chrome', chrome);
	});

	it('keeps each storage area apart, and reads keys as Chrome does', async () => {
		const { storage } = installChrome(PAGE);
		await storage.local.set({ marks: ['/a'], theme: 'dark' });
		await storage.session.set({ queue: [] });

		expect(await storage.local.get()).toEqual({ marks: ['/a'], theme: 'dark' });
		expect(await storage.local.get('marks')).toEqual({ marks: ['/a'] });
		expect(await storage.local.get(['theme', 'missing'])).toEqual({ theme: 'dark' });

		expect(await storage.session.get(null)).toEqual({ queue: [] });

		await storage.local.remove('theme');
		await storage.local.remove(['marks']);

		expect(await storage.local.get()).toEqual({});
		await expect(storage.session.setAccessLevel()).resolves.toBeUndefined();
	});

	it('hands out copies, so what a reader changes is not what is kept', async () => {
		const { storage } = installChrome(PAGE);
		const marks = ['/a'];
		await storage.local.set({ marks });
		marks.push('/b');
		const read = await storage.local.get('marks');

		if (Array.isArray(read.marks)) read.marks.push('/c');

		expect(await storage.local.get('marks')).toEqual({ marks: ['/a'] });
	});

	it('tells onChanged what changed, in which area, until it stops listening', async () => {
		const { storage } = installChrome(PAGE);
		const heard: unknown[][] = [];
		const listener = (...args: unknown[]) => void heard.push(args);
		storage.onChanged.addListener(listener);

		await storage.session.set({ queue: ['/a'] });
		await storage.session.set({ queue: ['/b'] });
		storage.onChanged.removeListener(listener);
		await storage.session.set({ queue: [] });

		expect(heard).toEqual([
			[{ queue: { oldValue: undefined, newValue: ['/a'] } }, 'session'],
			[{ queue: { oldValue: ['/a'], newValue: ['/b'] } }, 'session']
		]);
	});

	it('answers for the playlist tab, as its content script would', async () => {
		const { tabs, runtime, downloads } = installChrome(PAGE);

		expect(await tabs.query()).toEqual([PAGE.tab]);
		expect(await tabs.get()).toEqual(PAGE.tab);
		expect(await tabs.sendMessage(7, { type: 'EXTRACT_TRACKS' })).toEqual(PAGE.answers.EXTRACT_TRACKS);
		expect(await tabs.sendMessage(7, { type: 'NOT_A_QUESTION' })).toBeUndefined();
		expect(await tabs.create({ url: 'https://soundcloud.com/' })).toEqual({ id: 8, url: 'https://soundcloud.com/' });
		await expect(tabs.remove()).resolves.toBeUndefined();
		await expect(runtime.sendMessage()).resolves.toBeUndefined();
		expect(await downloads.download()).toBe(1);
	});
});

describe('PAGE', () => {
	it('says where every track can be downloaded, and who every artist is', () => {
		const { EXTRACT_TRACKS, GET_DOWNLOAD_DETAILS, GET_ARTIST_PROFILES } = PAGE.answers;
		const permalinks = EXTRACT_TRACKS.tracks.map((track) => track.permalink);
		const artists = permalinks.map((path) => path.split('/').slice(0, 2).join('/'));

		expect(new Set(permalinks).size).toBe(10);
		expect(GET_DOWNLOAD_DETAILS.tracks.map((track) => track.details.permalink)).toEqual(permalinks);
		expect(GET_ARTIST_PROFILES.profiles.map((profile) => profile.permalink)).toEqual(artists);
	});
});

describe('cover', () => {
	it('is the same picture for the same artwork, and another for another', () => {
		expect(cover('https://i1.sndcdn.com/a.jpg')).toBe(cover('https://i1.sndcdn.com/a.jpg'));
		expect(cover('https://i1.sndcdn.com/a.jpg')).not.toBe(cover('https://i1.sndcdn.com/b.jpg'));
		expect(cover('x')).toMatch(/^<svg xmlns="http:\/\/www.w3.org\/2000\/svg"/);
	});
});
