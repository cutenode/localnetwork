// src/lib/queue.svelte.ts — the session queue.
import { describe, it, expect, beforeEach, vi } from 'vitest';
import { flush } from '../helpers/flush.ts';
import { createChrome, type ChromeMock, type Seed } from '../helpers/chrome.ts';
import type { Overrides } from '../helpers/state-fixtures.ts';

declare const chrome: ChromeMock;

const track = (permalink: string, extra: Overrides = {}) => ({
	permalink,
	title: 'Title',
	artist: 'Artist',
	artworkUrl: 'https://i1.sndcdn.com/artworks-abc-t120x120.jpg',
	// What only the page a track came off could say; a queued entry keeps all
	// three, so the Manager's list still has them when it adds from the queue.
	genre: null,
	liked: null,
	reposted: null,
	...extra
});

// A fresh module graph each time, so the singleton starts empty.
async function freshQueue(session = {}) {
	vi.resetModules();
	globalThis.chrome = createChrome({ session });
	const { queue } = await import('../../src/lib/queue.svelte.ts');
	await queue.start();

	return queue;
}

const stored = () => chrome.testStore('session').queue;

beforeEach(() => {
	vi.setSystemTime(new Date('2026-09-08T12:00:00Z'));
});

describe('the queue', () => {
	it('reads what the session holds, and says what it holds', async () => {
		const queue = await freshQueue({ queue: [track('/a/one')] });
		expect(queue.count).toBe(1);
		expect(queue.permalinks).toEqual(['/a/one']);
		expect(queue.has('/a/one')).toBe(true);
		expect(queue.has('/b/two')).toBe(false);
	});

	it('starts empty when the session holds nothing', async () => {
		const queue = await freshQueue();
		expect(queue.items).toEqual([]);
	});

	it('is read once, however many panels ask', async () => {
		const queue = await freshQueue();
		expect(chrome.storage.session.get).toHaveBeenCalledTimes(1);
		await queue.start();
		expect(chrome.storage.session.get).toHaveBeenCalledTimes(1);
		expect(chrome.storage.onChanged.addListener).toHaveBeenCalledTimes(1);
	});

	it('keeps of a track only what the tools that read the queue need', async () => {
		const queue = await freshQueue();
		const details = { purchaseUrl: 'https://hypeddit.com/one' };
		await queue.add([
			track('/a/one', { details, genre: 'Techno', liked: true, domPurchaseTitle: 'Buy', extra: 'ignored' })
		]);
		expect(stored()).toEqual([
			{
				permalink: '/a/one',
				title: 'Title',
				artist: 'Artist',
				artworkUrl: 'https://i1.sndcdn.com/artworks-abc-t120x120.jpg',
				details,
				// Kept, because nothing else can say them: the page is gone by the
				// time a queued track is added to the Manager's list.
				genre: 'Techno',
				liked: true,
				reposted: null
			}
		]);
	});

	// A queue written before those three were kept — or by a page whose rows
	// said nothing about them — reads back as "nobody knows" rather than as a
	// claim about the track.
	it('says nobody knows where a track carries none of the three', async () => {
		const queue = await freshQueue();
		await queue.add([{ permalink: '/a/one', title: 'Title', artist: 'Artist', artworkUrl: '' }]);
		expect(stored()[0]).toMatchObject({ genre: null, liked: null, reposted: null });
	});

	// What the exported state file is written from (state-transfer.js): the
	// panel hands these over rather than letting the file read session storage,
	// so a queue held only in memory is exported like any other.
	it('hands over what it holds as plain data', async () => {
		const queue = await freshQueue({ queue: [track('/a/one')] });
		expect(queue.entries).toEqual([track('/a/one')]);
		// Plain, unlike the `$state` proxy behind `items`, which is the whole
		// point: a structured clone — every storage write, and the file the panel
		// serializes — refuses a proxy.
		expect(() => structuredClone(queue.items)).toThrow(/could not be cloned/);
		expect(structuredClone(queue.entries)).toEqual([track('/a/one')]);

		await queue.clear();
		expect(queue.entries).toEqual([]);
	});

	it('adds in the order given, and says how many were new', async () => {
		const queue = await freshQueue();
		expect(await queue.add([track('/a/one'), track('/b/two')])).toBe(2);
		// Already queued, a duplicate within the one call, and a track with no
		// permalink — nothing to tell it apart by, and nothing to mark later.
		expect(await queue.add([track('/a/one'), track('/c/three'), track('/c/three')])).toBe(1);
		expect(await queue.add([track('/a/one')])).toBe(0);
		expect(await queue.add([track('')])).toBe(0);
		expect(queue.permalinks).toEqual(['/a/one', '/b/two', '/c/three']);
		// The last two calls added nothing, so they wrote nothing.
		expect(chrome.storage.session.set).toHaveBeenCalledTimes(2);
	});

	it('takes one out, toggles, and empties', async () => {
		const queue = await freshQueue();
		await queue.add([track('/a/one'), track('/b/two')]);

		await queue.remove('/a/one');
		expect(queue.permalinks).toEqual(['/b/two']);

		await queue.toggle(track('/a/one'));
		expect(queue.permalinks).toEqual(['/b/two', '/a/one']);
		await queue.toggle(track('/a/one'));
		expect(queue.permalinks).toEqual(['/b/two']);

		await queue.clear();
		expect(queue.count).toBe(0);
		expect(stored()).toEqual([]);
	});

	it('follows a change made in another popup window, and ignores the rest', async () => {
		const queue = await freshQueue();
		chrome.testEmitStorageChange({ queue: { newValue: [track('/a/one')] } }, 'session');
		expect(queue.permalinks).toEqual(['/a/one']);

		// Another area, and another key of the same area: neither is the queue.
		chrome.testEmitStorageChange({ queue: { newValue: [track('/b/two')] } }, 'local');
		chrome.testEmitStorageChange({ downloaded: { newValue: {} } }, 'session');
		expect(queue.permalinks).toEqual(['/a/one']);

		chrome.testEmitStorageChange({ queue: { newValue: undefined } }, 'session');
		expect(queue.count).toBe(0);
	});

	// The content script paints the "Queued" pills on the SoundCloud page, and a
	// content script cannot read the session area until a trusted context opens
	// it. The popup is that context, and this is the only place the call is
	// made — started before anything is read or written, so a page open behind
	// the popup sees the queue from the first entry.
	it('opens the session area to the content script as the queue starts', async () => {
		const queue = await freshQueue({ queue: [track('/a/one')] });
		expect(chrome.storage.session.setAccessLevel).toHaveBeenCalledWith({
			accessLevel: 'TRUSTED_AND_UNTRUSTED_CONTEXTS'
		});
		expect(chrome.storage.session.setAccessLevel.mock.invocationCallOrder[0]).toBeLessThan(
			chrome.storage.session.get.mock.invocationCallOrder[0]
		);
		// Once, however many panels ask.
		await queue.start();
		expect(chrome.storage.session.setAccessLevel).toHaveBeenCalledTimes(1);
	});

	// Two round trips to the browser process, and the read does not depend on
	// the door being open: the popup would otherwise wait through both before
	// it could show what is queued.
	it('reads the queue without waiting for the area to be opened', async () => {
		vi.resetModules();
		globalThis.chrome = createChrome({ session: { queue: [track('/a/one')] } });
		let openDoor!: () => void;
		chrome.storage.session.setAccessLevel.mockReturnValue(new Promise((resolve) => (openDoor = resolve)));
		const { queue } = await import('../../src/lib/queue.svelte.ts');

		await queue.start();
		expect(queue.permalinks).toEqual(['/a/one']);

		// A write is the one thing that does wait: landing in an area the content
		// script cannot read is what would cost the page its pills.
		let written = false;
		void queue.add([track('/b/two')]).then(() => (written = true));
		await flush();
		expect(written).toBe(false);
		// On screen all the same — the panel shows what the queue holds.
		expect(queue.permalinks).toEqual(['/a/one', '/b/two']);

		openDoor();
		await flush();
		expect(written).toBe(true);
		expect(stored().map((entry) => entry.permalink)).toEqual(['/a/one', '/b/two']);
	});

	it('reads the queue anyway when the session area cannot be opened', async () => {
		vi.resetModules();
		globalThis.chrome = createChrome({ session: { queue: [track('/a/one')] } });
		chrome.storage.session.setAccessLevel.mockRejectedValue(new Error('not a function'));
		const { queue } = await import('../../src/lib/queue.svelte.ts');
		await queue.start();
		// The popup is unharmed; the page is the half that goes without its pills.
		expect(queue.permalinks).toEqual(['/a/one']);
	});

	it('starts empty, and stops following, when the session cannot be read', async () => {
		vi.resetModules();
		// A mirror from an earlier popup, which the queue shows until the session
		// answers — and which is only a guess about a session there is none of.
		localStorage.setItem('queue:items', JSON.stringify([track('/a/one')]));
		globalThis.chrome = createChrome();
		chrome.storage.session.get.mockRejectedValue(new Error('Extension context invalidated'));
		const { queue } = await import('../../src/lib/queue.svelte.ts');
		expect(queue.count).toBe(1);
		await queue.start();
		expect(queue.items).toEqual([]);
		expect(localStorage.getItem('queue:items')).toBe(null);
		expect(chrome.storage.onChanged.addListener).not.toHaveBeenCalled();
	});

	it('holds the queue anyway when the session cannot be written', async () => {
		const queue = await freshQueue();
		chrome.storage.session.set.mockRejectedValue(new Error('Extension context invalidated'));
		expect(await queue.add([track('/a/one')])).toBe(1);
		await flush();
		// Lost when the popup closes, but the panel that queued it still shows it.
		expect(queue.permalinks).toEqual(['/a/one']);
		expect(stored()).toBeUndefined();
	});
});

// The mirror in the popup's own localStorage: what makes the queue there on
// the first turn the popup runs rather than a browser round trip later. The
// header of every view is drawn from `count`, and a popup that is destroyed on
// every click that opens a tab draws that header a great many times.
describe('the mirror of the queue', () => {
	const MIRROR = 'queue:items';

	const mirror = () => JSON.parse(localStorage.getItem(MIRROR) ?? 'null');

	/** Imported but not started: the queue as the popup's first line sees it. */
	async function unstarted(session = {}) {
		vi.resetModules();
		globalThis.chrome = createChrome({ session });
		const { queue } = await import('../../src/lib/queue.svelte.ts');

		return queue;
	}

	it('answers with what was queued before the session has been asked', async () => {
		localStorage.setItem(MIRROR, JSON.stringify([track('/a/one'), track('/b/two')]));
		const queue = await unstarted({ queue: [track('/a/one'), track('/b/two')] });

		expect(queue.count).toBe(2);
		expect(queue.items).toEqual([track('/a/one'), track('/b/two')]);
		expect(queue.permalinks).toEqual(['/a/one', '/b/two']);
		expect(queue.has('/a/one')).toBe(true);
		expect(chrome.storage.session.get).not.toHaveBeenCalled();
	});

	it('is written on every change, by whichever end made it', async () => {
		const queue = await freshQueue();
		await queue.add([track('/a/one')]);
		expect(mirror()).toEqual([track('/a/one')]);

		await queue.add([track('/b/two')]);
		expect(mirror()).toEqual([track('/a/one'), track('/b/two')]);

		await queue.remove('/a/one');
		expect(mirror()).toEqual([track('/b/two')]);

		// The queue button on a SoundCloud page, or another window's popup: what
		// the change event carries is mirrored the same way.
		chrome.testEmitStorageChange({ queue: { newValue: [track('/c/three')] } }, 'session');
		expect(mirror()).toEqual([track('/c/three')]);

		await queue.clear();
		// Nothing queued is no mirror at all, rather than a mirror of nothing.
		expect(localStorage.getItem(MIRROR)).toBe(null);
	});

	it('is only ever a guess, and the session is what settles it', async () => {
		localStorage.setItem(MIRROR, JSON.stringify([track('/a/one'), track('/b/two')]));
		const queue = await unstarted({ queue: [track('/c/three')] });
		expect(queue.permalinks).toEqual(['/a/one', '/b/two']);

		await queue.start();
		expect(queue.permalinks).toEqual(['/c/three']);
		expect(mirror()).toEqual([track('/c/three')]);
	});

	// localStorage outlives a browsing session and chrome.storage.session does
	// not, so this is what every first opening of a new one looks like.
	it('is emptied when a new browsing session finds the queue gone', async () => {
		localStorage.setItem(MIRROR, JSON.stringify([track('/a/one')]));
		const queue = await unstarted();
		expect(queue.count).toBe(1);

		await queue.start();
		expect(queue.items).toEqual([]);
		expect(localStorage.getItem(MIRROR)).toBe(null);
	});

	it('drops an entry a version that kept more once wrote, as the session’s is dropped', async () => {
		localStorage.setItem(MIRROR, JSON.stringify([track('/a/one'), { title: 'no permalink' }, null, 'not an entry']));
		expect((await unstarted()).items).toEqual([track('/a/one')]);
	});

	it('is ignored when it holds something that is not a queue', async () => {
		for (const held of ['not json at all', '{"queue":1}', 'null', '[]']) {
			localStorage.setItem(MIRROR, held);
			expect((await unstarted()).items, held).toEqual([]);
		}
	});

	// The read is a round trip, and the mirror is there so that something can be
	// queued before it lands. What is queued in that window is newer than the
	// answer coming back, and is already in the session: taking the answer would
	// undo it on screen and leave it written.
	it('keeps what was queued while the session was still being read', async () => {
		vi.resetModules();
		globalThis.chrome = createChrome({ session: { queue: [track('/a/one')] } });
		let answer!: (items: Seed) => void;
		chrome.storage.session.get.mockReturnValue(new Promise((resolve) => (answer = resolve)));
		const { queue } = await import('../../src/lib/queue.svelte.ts');

		const reading = queue.start();
		await queue.add([track('/b/two')]);
		answer({ queue: [track('/a/one')] });
		await reading;

		expect(queue.permalinks).toEqual(['/b/two']);
		expect(mirror()).toEqual([track('/b/two')]);
		// And the session is still followed: the popup is not cut off from the
		// page because of what it did while the read was out.
		chrome.testEmitStorageChange({ queue: { newValue: [track('/c/three')] } }, 'session');
		expect(queue.permalinks).toEqual(['/c/three']);
	});

	it('keeps it when the session cannot be read at all either', async () => {
		vi.resetModules();
		globalThis.chrome = createChrome();
		let refuse!: (error: Error) => void;
		chrome.storage.session.get.mockReturnValue(new Promise((_r, reject) => (refuse = reject)));
		const { queue } = await import('../../src/lib/queue.svelte.ts');

		const reading = queue.start();
		await queue.add([track('/b/two')]);
		refuse(new Error('Extension context invalidated'));
		await reading;

		expect(queue.permalinks).toEqual(['/b/two']);
	});

	// A popup with no localStorage to reach — a `SecurityError` on every call,
	// which is what a browser with site data switched off answers with. The
	// queue is exactly what it was before there was a mirror.
	it('costs the queue nothing when there is no localStorage to keep it in', async () => {
		const fails = () => {
			throw new DOMException('The operation is insecure.', 'SecurityError');
		};

		vi.stubGlobal('localStorage', {
			getItem: fails,
			setItem: fails,
			removeItem: fails,
			clear: () => {},
			key: () => null,
			length: 0
		});

		const queue = await freshQueue({ queue: [track('/a/one')] });
		expect(queue.permalinks).toEqual(['/a/one']);
		expect(await queue.add([track('/b/two')])).toBe(1);
		expect(queue.permalinks).toEqual(['/a/one', '/b/two']);
		await flush();
		expect(stored()).toEqual([track('/a/one'), track('/b/two')]);

		await queue.clear();
		expect(queue.items).toEqual([]);
		vi.unstubAllGlobals();
	});

	// A queue too long for what localStorage has left. The one thing that must
	// not happen is the stale mirror staying behind to be drawn next time.
	it('drops the mirror rather than leave a stale one when it cannot be written', async () => {
		const queue = await freshQueue();
		await queue.add([track('/a/one')]);
		expect(mirror()).toEqual([track('/a/one')]);

		const full = vi.spyOn(Storage.prototype, 'setItem').mockImplementation(() => {
			throw new DOMException('exceeded the quota.', 'QuotaExceededError');
		});

		await queue.add([track('/b/two')]);
		full.mockRestore();

		expect(localStorage.getItem(MIRROR)).toBe(null);
		// The queue itself is untouched: the session has it, and so does the popup.
		expect(queue.permalinks).toEqual(['/a/one', '/b/two']);
		expect(stored()).toEqual([track('/a/one'), track('/b/two')]);
	});
});

// An imported state file is restored through this store and nothing else: the
// settings sheet (StateTransfer.svelte) hands `doc.queue` to `add`, or to
// `replace` when the import is asked to replace, and state-transfer.js does
// not write the queue at all. So the guarantee that a re-import cannot double
// the queue up is this store's, and these are the cases it has to hold in — a
// track already queued, a file naming one twice, and the same file read again
// tomorrow.
describe('the queue an imported file is restored into', () => {
	// An entry as a file carries one, which may well name a track that is
	// queued already — the file was exported from a queue like this one.
	const fromFile = (permalink: string, title = 'from the file') => track(permalink, { title, details: null });

	it('adds nothing for a track it already holds, and leaves that entry as it was', async () => {
		const queue = await freshQueue();
		await queue.add([track('/a/one', { title: 'as it was queued' })]);
		chrome.storage.session.set.mockClear();

		expect(await queue.add([fromFile('/a/one')])).toBe(0);
		// The entry on screen wins: the file's title and its details are dropped
		// with the rest of the duplicate.
		expect(queue.items).toEqual([track('/a/one', { title: 'as it was queued' })]);
		expect(chrome.storage.session.set).not.toHaveBeenCalled();
	});

	// The schema and the importer both accept a file listing a track twice —
	// JSON Schema cannot say "unique by key", and the state-schema test pins
	// that as valid — so this is where the second one is dropped.
	it('queues a track the file names twice only once', async () => {
		const queue = await freshQueue();
		expect(await queue.add([fromFile('/a/one', 'once'), fromFile('/a/one', 'again')])).toBe(1);
		expect(queue.items).toEqual([fromFile('/a/one', 'once')]);
	});

	it('appends what is new in the file’s order, behind what is queued now', async () => {
		const queue = await freshQueue({ queue: [track('/a/one'), track('/b/two')] });

		const doc = [fromFile('/b/two'), fromFile('/c/three'), fromFile('/d/four')];
		expect(await queue.add(doc)).toBe(2);
		expect(queue.permalinks).toEqual(['/a/one', '/b/two', '/c/three', '/d/four']);
		// What was queued is still at the front, in order and untouched.
		expect(queue.items.slice(0, 2)).toEqual([track('/a/one'), track('/b/two')]);
		expect(queue.items.slice(2)).toEqual([fromFile('/c/three'), fromFile('/d/four')]);
	});

	it('is the same queue however often the same file is imported', async () => {
		const queue = await freshQueue();
		const doc = [fromFile('/a/one'), fromFile('/b/two')];

		expect(await queue.add(doc)).toBe(2);
		const once = queue.entries;
		expect(await queue.add(doc)).toBe(0);
		expect(await queue.add(doc)).toBe(0);
		expect(queue.entries).toEqual(once);
	});

	// The other half of an import, which nobody reaches without turning a
	// switch on: the queue becomes the file's queue and what was in it is gone.
	it('becomes exactly the file’s queue when the import is asked to replace', async () => {
		const queue = await freshQueue({ queue: [track('/a/one'), track('/b/two')] });

		expect(await queue.replace([fromFile('/b/two'), fromFile('/c/three')])).toBe(2);
		expect(queue.permalinks).toEqual(['/b/two', '/c/three']);
		// The file's entry, not the one that was queued: replacing means the file
		// is what is left, down to the title on a track both of them name.
		expect(queue.items).toEqual([fromFile('/b/two'), fromFile('/c/three')]);
		expect(stored()).toEqual([fromFile('/b/two'), fromFile('/c/three')]);
	});

	it('still holds a track the file names twice only once, and empties for an empty file', async () => {
		const queue = await freshQueue({ queue: [track('/a/one')] });
		expect(await queue.replace([fromFile('/b/two'), fromFile('/b/two', 'again')])).toBe(1);
		expect(queue.permalinks).toEqual(['/b/two']);

		expect(await queue.replace([])).toBe(0);
		expect(queue.items).toEqual([]);
		expect(stored()).toEqual([]);
	});

	it('restores into the queue it is holding in memory, unwritable session or not', async () => {
		const queue = await freshQueue();
		chrome.storage.session.set.mockRejectedValue(new Error('Extension context invalidated'));
		await queue.add([track('/a/one')]);
		await flush();
		expect(stored()).toBeUndefined();

		// Nothing was ever written, so nothing can be read back — and the queue
		// deduplicates against what it is showing all the same.
		expect(await queue.add([fromFile('/a/one'), fromFile('/b/two')])).toBe(1);
		expect(queue.permalinks).toEqual(['/a/one', '/b/two']);
	});
});
