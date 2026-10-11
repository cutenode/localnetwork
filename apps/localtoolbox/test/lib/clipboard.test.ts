// src/lib/clipboard.ts — copying out of the popup, and offering a file.
import { describe, it, expect, vi } from 'vitest';
import { copyText, copyImage, downloadText } from '../../src/lib/clipboard.ts';
import { installClipboard, installDownload } from '../helpers/panel.ts';

describe('copyText', () => {
	it('uses the async clipboard when it can', async () => {
		const clipboard = installClipboard();
		expect(await copyText('hello')).toBe(true);
		expect(clipboard.writeText).toHaveBeenCalledWith('hello');
	});

	it('falls back to a selection when the clipboard is not available at all', async () => {
		// No navigator.clipboard: reading it throws, as it does on an insecure
		// origin.
		Object.defineProperty(navigator, 'clipboard', { value: undefined, configurable: true });
		let selected = '';
		document.execCommand = vi.fn<typeof document.execCommand>(() => {
			selected = document.querySelector('textarea')!.value;

			return true;
		});
		expect(await copyText('hello')).toBe(true);
		expect(selected).toBe('hello');
		// The textarea it selected in is gone again.
		expect(document.querySelector('textarea')).toBe(null);
		// @ts-expect-error -- takes the stub above away; the DOM types never lack an execCommand
		delete document.execCommand;
	});

	it('says so when neither route works', async () => {
		const clipboard = installClipboard();
		clipboard.writeText.mockRejectedValue(new Error('not focused'));
		expect(await copyText('hello')).toBe(false);
	});
});

// jsdom has none of what the image route runs on — no `createImageBitmap`, no
// `OffscreenCanvas`, no `ClipboardItem` — so the decode-and-re-encode pipeline
// is stood up here. Every piece goes in through `vi.stubGlobal`, which
// `unstubGlobals` in vitest.config.ts takes away again after the test: the
// suite is run shuffled, and a global left lying about is a later test passing
// on this one's leftovers.
function installImagePipeline() {
	const jpeg = new Blob(['jpeg bytes'], { type: 'image/jpeg' });
	const png = new Blob(['png bytes'], { type: 'image/png' });
	const bitmap = { width: 1080, height: 1080 };
	const drawn: unknown[][] = [];

	const canvas = {
		width: 0,
		height: 0,
		getContext: vi.fn<() => { drawImage: (...args: unknown[]) => void }>(() => ({
			drawImage: (...args) => {
				drawn.push(args);
			}
		})),
		convertToBlob: vi.fn<() => Promise<Blob>>(async () => png)
	};

	// What the image route reads of a fetch's answer.
	const fetchImage = vi.fn<(url: string) => Promise<{ ok: boolean; status: number; blob: () => Promise<Blob> }>>(
		async () => ({ ok: true, status: 200, blob: async () => jpeg })
	);

	vi.stubGlobal('fetch', fetchImage);
	vi.stubGlobal(
		'createImageBitmap',
		vi.fn<() => Promise<typeof bitmap>>(async () => bitmap)
	);
	// The constructor hands back the one canvas above, so a test can read what
	// was drawn on it; the size it was asked for is recorded on the way past.
	vi.stubGlobal('OffscreenCanvas', function (width: number, height: number) {
		return Object.assign(canvas, { width, height });
	});
	vi.stubGlobal(
		'ClipboardItem',
		class {
			declare items: Record<string, Blob>;

			constructor(items: Record<string, Blob>) {
				this.items = items;
			}
		}
	);

	return { clipboard: installClipboard(), fetch: fetchImage, canvas, drawn, bitmap, jpeg, png };
}

const ARTWORK = 'https://i1.sndcdn.com/artworks-abc-t1080x1080.jpg';

describe('copyImage', () => {
	it('re-encodes the artwork and hands the clipboard a PNG', async () => {
		const art = installImagePipeline();
		expect(await copyImage(ARTWORK)).toBe(true);
		expect(fetch).toHaveBeenCalledWith(ARTWORK);

		// The canvas is the decoded image's own size, so the cover is copied at
		// the resolution it was fetched at rather than at a default canvas's.
		expect([art.canvas.width, art.canvas.height]).toEqual([1080, 1080]);
		expect(art.drawn).toEqual([[art.bitmap, 0, 0]]);

		// The point of the round trip: Chrome's async clipboard takes PNG only,
		// so what is written is the re-encoded blob and never the fetched JPEG.
		expect(art.canvas.convertToBlob).toHaveBeenCalledWith({ type: 'image/png' });
		const [items] = art.clipboard.write.mock.calls[0];
		expect(items).toHaveLength(1);
		expect(items[0]).toHaveProperty('items', { 'image/png': art.png });
	});

	it('gives up on artwork the CDN will not serve', async () => {
		const art = installImagePipeline();
		// SoundCloud's CDN answers 404 for a size a track does not have, and a
		// 404 body would decode into nothing worth putting on the clipboard.
		art.fetch.mockResolvedValue({ ok: false, status: 404, blob: async () => art.jpeg });
		expect(await copyImage(ARTWORK)).toBe(false);
		expect(art.clipboard.write).not.toHaveBeenCalled();
	});

	it('gives up when the fetch itself never gets there', async () => {
		const art = installImagePipeline();
		art.fetch.mockRejectedValue(new TypeError('Failed to fetch'));
		expect(await copyImage(ARTWORK)).toBe(false);
		expect(art.clipboard.write).not.toHaveBeenCalled();
	});

	it('says so when the clipboard refuses the write', async () => {
		// The same lost focus that `copyText` recovers from, which an image
		// cannot: there is no selection route for a picture, so the row is simply
		// told it failed.
		const art = installImagePipeline();
		art.clipboard.write.mockRejectedValue(new Error('Document is not focused'));
		expect(await copyImage(ARTWORK)).toBe(false);
	});
});

describe('downloadText', () => {
	it('offers the text as a named file and lets go of the object URL', async () => {
		vi.useFakeTimers();
		const { link, clicked, createObjectURL, revokeObjectURL } = installDownload('blob:test/1');

		expect(downloadText('a,b', 'list.csv', 'text/csv')).toBe('list.csv');
		expect(link.download).toBe('list.csv');
		expect(link.href).toBe('blob:test/1');
		expect(clicked).toHaveBeenCalled();
		expect(createObjectURL.mock.calls[0][0].type).toBe('text/csv');

		expect(revokeObjectURL).not.toHaveBeenCalled();
		await vi.advanceTimersByTimeAsync(1000);
		expect(revokeObjectURL).toHaveBeenCalledWith('blob:test/1');
	});

	it('defaults to plain text', () => {
		const { createObjectURL } = installDownload('blob:test/2');
		downloadText('plain', 'notes.txt');
		expect(createObjectURL.mock.calls[0][0].type).toBe('text/plain');
	});
});
