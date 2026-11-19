import { describe, expect, it } from 'vitest';
import { eventPagePath, spacePagePath } from './space-paths.ts';

describe('spacePagePath', () => {
	it('puts the key first and the name after it, as an address part', () => {
		expect(spacePagePath('venue', '3lbx2kz', 'Panorama Bar')).toBe('/venue/3lbx2kz/panorama-bar');
		expect(spacePagePath('party', '3lbx2kz', 'Ñoño & Friends!')).toBe('/party/3lbx2kz/nono-and-friends');
		expect(spacePagePath('act', '3lbx2kz', 'X')).toBe('/act/3lbx2kz/x');
	});

	it('leaves just the key when the name has nothing to keep', () => {
		expect(spacePagePath('venue', '3lbx2kz', '東京')).toBe('/venue/3lbx2kz');
		expect(spacePagePath('act', '3lbx2kz', '!!')).toBe('/act/3lbx2kz');
	});

	it("leaves just the key for a name that reads as where a party's next event is made", () => {
		expect(spacePagePath('party', '3lbx2kz', 'New')).toBe('/party/3lbx2kz');
		expect(spacePagePath('party', '3lbx2kz', 'New Moon')).toBe('/party/3lbx2kz/new-moon');
	});
});

describe('eventPagePath', () => {
	it("puts the event's key under its party's, then its name", () => {
		expect(eventPagePath('3lbx2kz', '3mabcdefghijk', 'Winter Opening')).toBe(
			'/party/3lbx2kz/3mabcdefghijk/winter-opening'
		);
		expect(eventPagePath('3lbx2kz', '3mabcdefghijk', '!!')).toBe('/party/3lbx2kz/3mabcdefghijk');
	});
});
