// src/tools/third-party-downloads/save.ts — naming a file, telling a file from
// a page, and handing one to the browser.
import { describe, it, expect } from 'vitest';
import { filenameOf, looksLikeFile, safeName, save } from '../../src/tools/third-party-downloads/save.ts';
import type { ChromeMock } from '../helpers/chrome.ts';

declare const chrome: ChromeMock;

const signed = (disposition: string) =>
	`https://gates.s3.amazonaws.com/886rp1?response-content-disposition=${encodeURIComponent(
		disposition
	)}&X-Amz-Signature=abc`;

describe('safeName', () => {
	it('makes one path segment a browser will take', () => {
		expect(safeName('a/b\\c.wav')).toBe('a-b-c.wav');
		expect(safeName('..hidden.mp3 ')).toBe('hidden.mp3');
		expect(safeName(`${'x'.repeat(300)}.wav`)).toHaveLength(180);
	});

	// Chrome refuses the whole download over any of these rather than cleaning
	// the name up, and artists name their files with all of them.
	it('takes out what Chrome calls an invalid filename', () => {
		expect(safeName('Lose My Breath (baile flip) *FREE DL*.mp3')).toBe('Lose My Breath (baile flip) -FREE DL-.mp3');
		expect(safeName('Remix? "Radio" Edit <v2>|final:.mp3')).toBe('Remix- -Radio- Edit -v2--final-.mp3');
		expect(safeName('trailing dots...')).toBe('trailing dots');
		expect(safeName('NUL.mp3')).toBe('_NUL.mp3');
	});

	// The kind that survives a copy out of a page and is never visible in it.
	it('takes out the characters nobody can see', () => {
		expect(safeName('zero\u200bwidth\u00adsoft.mp3')).toBe('zerowidthsoft.mp3');
		expect(safeName('bidi\u202emarked.mp3')).toBe('bidimarked.mp3');
	});

	// Cutting a long name to length can land in the middle of a character, and
	// half a character is a name Chrome will not take either.
	it('does not leave half a character behind when it cuts', () => {
		const cut = safeName(`${'x'.repeat(179)}\u{1f3b5}.mp3`);
		expect(cut).toBe('x'.repeat(179));
		expect(safeName(cut)).toBe(cut);
	});
});

describe('filenameOf', () => {
	it('prefers the name signed into the link', () => {
		expect(filenameOf(signed('attachment; filename="KAKU - FE!N (Remix).wav"'))).toBe('KAKU - FE!N (Remix).wav');
		expect(filenameOf(signed("attachment; filename*=UTF-8''Caf%C3%A9.mp3"))).toBe('Café.mp3');
	});

	it('falls back to the last path segment when it is a file', () => {
		expect(filenameOf('https://cdn.test/tracks/My%20Track.flac?x=1')).toBe('My Track.flac');
		expect(filenameOf('https://cdn.test/tracks/thing')).toBe('');
		expect(filenameOf('https://cdn.test/album/')).toBe('');
	});

	// The segment is decoded before it is used, so a separator the link carried
	// encoded is a real one by then — and a name with one in it is a path.
	it('makes a decoded segment one name, whatever separators it decodes to', () => {
		expect(filenameOf('https://cdn.test/a/..%2F..%2Fevil.mp3')).toBe('-..-evil.mp3');
		expect(filenameOf('https://cdn.test/a/..%5Cwin.wav')).toBe('-win.wav');
	});

	it('keeps a name that will not decode rather than losing it', () => {
		expect(filenameOf(signed('attachment; filename="100%.wav"'))).toBe('100%.wav');
		expect(filenameOf('https://cdn.test/100%.wav')).toBe('100%.wav');
	});

	it('answers nothing at all for something that is not a URL', () => {
		expect(filenameOf('not a url')).toBe('');
	});
});

describe('looksLikeFile', () => {
	it('knows the extensions a free download arrives as', () => {
		for (const ext of ['mp3', 'wav', 'aiff', 'aif', 'flac', 'm4a', 'ogg', 'zip']) {
			expect(looksLikeFile(`https://cdn.test/a.${ext}`), ext).toBe(true);
		}

		expect(looksLikeFile('https://cdn.test/a.html')).toBe(false);
	});

	it('takes Dropbox and Drive only when asked for the bytes', () => {
		expect(looksLikeFile('https://www.dropbox.com/s/abc/track?dl=1')).toBe(true);
		expect(looksLikeFile('https://www.dropbox.com/s/abc/track?dl=0')).toBe(false);
		expect(looksLikeFile('https://drive.google.com/uc?id=1&export=download')).toBe(true);
		expect(looksLikeFile('https://drive.google.com/file/d/1/view')).toBe(false);
	});

	it('says no to anything that is not a URL', () => {
		expect(looksLikeFile('nope')).toBe(false);
	});
});

describe('save', () => {
	const downloads = () => {
		const { download } = chrome.downloads;
		download.mockResolvedValue(42);

		return download;
	};

	it('puts a file it can name in the site’s own folder', async () => {
		const download = downloads();
		const url = signed('attachment; filename="One.wav"');
		expect(await save(url, { subfolder: 'Droploud' })).toEqual({ id: 42, name: 'One.wav' });
		expect(download).toHaveBeenCalledWith({ url, filename: 'Droploud/One.wav' });
	});

	it('takes a name the site gave it over the one in the link', async () => {
		const download = downloads();
		await save('https://cdn.test/9f3a1c.wav', { subfolder: 'ToneDen', filename: 'Real Name.wav' });
		expect(download).toHaveBeenCalledWith({
			url: 'https://cdn.test/9f3a1c.wav',
			filename: 'ToneDen/Real Name.wav'
		});
	});

	it('names it without a folder when there is no folder to use', async () => {
		const download = downloads();
		await save('https://cdn.test/One.wav');
		expect(download).toHaveBeenCalledWith({
			url: 'https://cdn.test/One.wav',
			filename: 'One.wav'
		});
	});

	it('leaves the name to Chrome when the link does not carry one', async () => {
		const download = downloads();
		expect(await save('https://cdn.test/download', { subfolder: 'Droploud' })).toEqual({
			id: 42,
			name: ''
		});
		expect(download).toHaveBeenCalledWith({ url: 'https://cdn.test/download' });
	});

	// safeName covers the rules Chrome documents, not every rule it has. A name
	// it refuses anyway is not worth the file, so the file is asked for again
	// without one and lands under whatever the response calls it.
	it('saves the file anyway when Chrome will not take the name', async () => {
		const download = downloads();
		download.mockRejectedValueOnce(new Error('Invalid filename'));
		const url = 'https://cdn.test/9f3a1c.wav';

		expect(await save(url, { subfolder: 'Hypeddit', filename: 'Lose My Breath.mp3' })).toEqual({
			id: 42,
			name: '',
			renamed: true
		});
		expect(download).toHaveBeenNthCalledWith(1, { url, filename: 'Hypeddit/Lose My Breath.mp3' });
		expect(download).toHaveBeenNthCalledWith(2, { url });
	});

	it('lets everything else fail as itself, without asking twice', async () => {
		const download = downloads();
		download.mockRejectedValue(new Error('No room on the disk.'));
		await expect(save('https://cdn.test/One.wav')).rejects.toThrow('No room on the disk.');
		expect(download).toHaveBeenCalledTimes(1);
	});

	// Nothing was named, so there is nothing to blame the refusal on: asking
	// again would only be the same call.
	it('does not ask twice when it never gave a name', async () => {
		const download = downloads();
		download.mockRejectedValue(new Error('Invalid filename'));
		await expect(save('https://cdn.test/download')).rejects.toThrow('Invalid filename');
		expect(download).toHaveBeenCalledTimes(1);
	});

	// The refusal is recognised by Chrome's own words for it, and words are all
	// there are to go on. A rejection carrying no message — a value that is not
	// an Error, or nothing thrown at all — cannot be that refusal however much
	// it looks like one, and asking again would drop a name Chrome never
	// objected to and hide whatever really went wrong.
	it('does not ask twice for a rejection that carries no message', async () => {
		const download = downloads();
		download.mockRejectedValueOnce('Invalid filename').mockRejectedValueOnce(undefined);

		await expect(save('https://cdn.test/One.wav')).rejects.toBe('Invalid filename');
		await expect(save('https://cdn.test/One.wav')).rejects.toBeUndefined();
		expect(download).toHaveBeenCalledTimes(2);
		expect(download).toHaveBeenCalledWith({ url: 'https://cdn.test/One.wav', filename: 'One.wav' });
	});

	// A title that was nothing but characters Chrome refuses comes out of
	// safeName empty, and the link still knows what the file is called.
	it('falls back to the name in the link when the site’s will not do', async () => {
		const download = downloads();
		const url = signed('attachment; filename="One.wav"');
		expect(await save(url, { subfolder: 'ToneDen', filename: '\u200b\u00ad' })).toEqual({
			id: 42,
			name: 'One.wav'
		});
		expect(download).toHaveBeenCalledWith({ url, filename: 'ToneDen/One.wav' });
	});
});
