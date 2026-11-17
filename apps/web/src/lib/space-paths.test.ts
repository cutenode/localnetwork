import { describe, expect, it } from 'vitest';
import { spacePagePath } from './space-paths.ts';

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
});
