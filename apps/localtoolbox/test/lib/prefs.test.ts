// src/lib/prefs.svelte.ts — options remembered in the popup's localStorage.
import { describe, it, expect, vi } from 'vitest';
import { persisted, flagPref, choicePref, reloadPrefs, textPref } from '../../src/lib/prefs.svelte.ts';

describe('persisted', () => {
	it('starts from the default and writes every change', () => {
		const pref = persisted('pref:x', 'a');
		expect(pref.value).toBe('a');
		pref.value = 'b';
		expect(pref.value).toBe('b');
		expect(localStorage.getItem('pref:x')).toBe('b');
	});

	it('starts from what was stored', () => {
		localStorage.setItem('pref:x', 'stored');
		expect(persisted('pref:x', 'a').value).toBe('stored');
	});

	it('re-reads the stored value on request, falling back to the default', () => {
		const pref = persisted('pref:x', 'a');
		localStorage.setItem('pref:x', 'elsewhere');
		pref.reload();
		expect(pref.value).toBe('elsewhere');

		localStorage.removeItem('pref:x');
		pref.reload();
		expect(pref.value).toBe('a');
	});

	it('keeps working when storage refuses to answer', () => {
		vi.spyOn(Storage.prototype, 'getItem').mockImplementation(() => {
			throw new Error('SecurityError');
		});
		vi.spyOn(Storage.prototype, 'setItem').mockImplementation(() => {
			throw new Error('QuotaExceededError');
		});
		const pref = persisted('pref:x', 'a');
		expect(pref.value).toBe('a');
		pref.value = 'b';
		expect(pref.value).toBe('b');
	});
});

// An import writes straight to localStorage, which no `$state` follows — and
// the panel whose options it just set is still on screen, under the settings
// sheet the import was run from. One call puts every option back in front of
// whoever is showing it.
describe('reloadPrefs', () => {
	it('re-reads every option there is, not only the one that changed', () => {
		const sort = choicePref('pref:sort', 'site', ['site', 'page']);
		const hide = flagPref('pref:hide-downloaded', false);
		const email = textPref('pref:email');

		localStorage.setItem('pref:sort', 'page');
		localStorage.setItem('pref:hide-downloaded', '1');
		expect(sort.value).toBe('site');

		reloadPrefs();
		expect(sort.value).toBe('page');
		expect(hide.value).toBe(true);
		// And one the import never touches is left where it was.
		expect(email.value).toBe('');
	});
});

describe('flagPref', () => {
	it('writes "1" and "0", which is what the exported state file carries', () => {
		const flag = flagPref('pref:flag', true);
		expect(flag.value).toBe(true);
		flag.value = false;
		expect(localStorage.getItem('pref:flag')).toBe('0');
		flag.value = true;
		expect(localStorage.getItem('pref:flag')).toBe('1');
	});

	it.each([
		['1', true],
		['0', false],
		// An older install wrote these.
		['true', true],
		['false', false],
		['nonsense', false]
	])('reads %o as %o', (stored, expected) => {
		localStorage.setItem('pref:flag', stored);
		expect(flagPref('pref:flag', true).value).toBe(expected);
	});
});

describe('choicePref', () => {
	it('takes a stored value that is on offer, and ignores one that is not', () => {
		localStorage.setItem('pref:sort', 'page');
		expect(choicePref('pref:sort', 'site', ['site', 'page']).value).toBe('page');

		localStorage.setItem('pref:sort', 'sideways');
		expect(choicePref('pref:sort', 'site', ['site', 'page']).value).toBe('site');
	});
});

describe('textPref', () => {
	it('stores what was typed without the space around it', () => {
		const email = textPref('pref:email');
		expect(email.value).toBe('');
		email.value = '  dj@example.com  ';
		expect(localStorage.getItem('pref:email')).toBe('dj@example.com');
		expect(textPref('pref:email').value).toBe('dj@example.com');
	});
});
