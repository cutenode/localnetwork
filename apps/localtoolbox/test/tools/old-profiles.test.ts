// src/tools/artists/old-profiles.ts: what the background worker does, on
// install, to a stored artists list an older version kept profiles on.
//
// Those profiles are other people's accounts, held for good. This version
// keeps them for the browser session only, so the clean-up takes them off the
// list on disk and leaves everything else about each entry as it was.
import { describe, it, expect } from 'vitest';
import { dropOldProfiles } from '../../src/tools/artists/old-profiles.ts';
import { createChrome, type ChromeMock, type Seed } from '../helpers/chrome.ts';

declare const chrome: ChromeMock;

const profile = { id: 7, permalink: '/a', username: 'A', city: 'Belfast', bio: 'hi', links: [], at: 1 };

const entry = (permalink: string, extra = {}) => ({
	permalink,
	name: 'A',
	tracks: [{ permalink: `${permalink}/one`, title: 'One' }],
	profile: null,
	from: 'My Playlist',
	at: 5,
	...extra
});

/** Runs the clean-up over a local area holding `storage`. */
async function dropFrom(storage: Seed) {
	globalThis.chrome = createChrome({ storage });
	await dropOldProfiles();
}

describe('dropping the profiles an older version kept', () => {
	it('nulls every profile on the list and keeps the rest of each entry, in one write', async () => {
		await dropFrom({
			artists: [
				entry('/a', { profile }),
				entry('/b'),
				entry('/c', { profile: { unavailable: true, at: 1 } }),
				null,
				'x'
			],
			listed: []
		});

		expect(chrome.storage.local.set).toHaveBeenCalledTimes(1);
		expect(chrome.testStore('local').artists).toEqual([
			entry('/a'),
			entry('/b'),
			entry('/c'),
			// Not entries at all, which is the store's to drop when it reads them.
			null,
			'x'
		]);
		expect(chrome.testStore('local').listed).toEqual([]);
	});

	it('writes nothing when no entry carries a profile', async () => {
		// Null, or no profile at all, as an entry out of a hand-made file may be.
		await dropFrom({ artists: [entry('/a'), { permalink: '/b', name: 'B' }, null, 7] });
		expect(chrome.storage.local.set).not.toHaveBeenCalled();
	});

	it('writes nothing when there is no list, or something else under its key', async () => {
		await dropFrom({});
		expect(chrome.storage.local.set).not.toHaveBeenCalled();

		await dropFrom({ artists: { '/a': entry('/a', { profile }) } });
		expect(chrome.storage.local.set).not.toHaveBeenCalled();
		expect(chrome.testStore('local').artists).toEqual({ '/a': entry('/a', { profile }) });
	});
});
