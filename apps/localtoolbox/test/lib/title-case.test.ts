// src/lib/title-case.ts — the transform behind the settings switch — and
// src/lib/title-case.svelte.ts, the switch itself.
//
// Two rules, written down where the file writes them down: a capital somebody
// typed is left alone, and a field entirely in capitals is re-cased because
// its capitals say nothing. Most of what is here is the second one not eating
// the first.
import { describe, it, expect, beforeEach } from 'vitest';
import { titleCase } from '../../src/lib/title-case.ts';
import { titleCasing } from '../../src/lib/title-case.svelte.ts';

describe('raising what was typed in lower case', () => {
	it('raises every word of a lower-case field', () => {
		expect(titleCase('glue')).toBe('Glue');
		expect(titleCase('midnight request line')).toBe('Midnight Request Line');
	});

	it('leaves the small words down in the middle of it', () => {
		expect(titleCase('fly me to the moon')).toBe('Fly Me to the Moon');
		expect(titleCase('a night at the opera')).toBe('A Night at the Opera');
	});

	// A title does not open or close in lower case, whatever the word is.
	it('raises a small word at either end anyway', () => {
		expect(titleCase('the')).toBe('The');
		expect(titleCase('the in between')).toBe('The in Between');
		expect(titleCase('what it is for')).toBe('What It Is For');
	});

	it('raises each part of a hyphenated or bracketed word', () => {
		expect(titleCase('hip-hop')).toBe('Hip-Hop');
		expect(titleCase('glue (original mix)')).toBe('Glue (Original Mix)');
		expect(titleCase('drum & bass')).toBe('Drum & Bass');
		expect(titleCase('acid/house')).toBe('Acid/House');
	});

	// The word is the letters, so an apostrophe inside one is carried along and
	// one standing outside it is a separator like any other.
	it('keeps apostrophes where they were', () => {
		expect(titleCase("don't stop")).toBe("Don't Stop");
		expect(titleCase("rock 'n' roll")).toBe("Rock 'N' Roll");
		expect(titleCase('o’clock')).toBe('O’clock');
	});

	it('leaves a field with no word in it exactly as it was', () => {
		expect(titleCase('')).toBe('');
		expect(titleCase(' — ')).toBe(' — ');
	});

	it('counts a digit as a word and leaves it alone', () => {
		expect(titleCase('808 state')).toBe('808 State');
	});
});

// Rule 1: a capital is a claim about the word, and nothing here can check one.
describe('capitals somebody typed', () => {
	it('leaves a word carrying one exactly as it was typed', () => {
		expect(titleCase('SBTRKT remix')).toBe('SBTRKT Remix');
		expect(titleCase('a DnB thing')).toBe('A DnB Thing');
		expect(titleCase('someone ft. McCoy')).toBe('Someone Ft. McCoy');
	});

	// Which means it only ever raises: a small word already raised stays up,
	// rather than this overruling somebody's own house style.
	it('never lowers one, small word or not', () => {
		expect(titleCase('The Best Of It')).toBe('The Best Of It');
		expect(titleCase('Glue')).toBe('Glue');
	});
});

// Rule 2: every letter a capital is no capitals at all.
describe('a field shouted in capitals', () => {
	it('re-cases it', () => {
		expect(titleCase('GLUE (ORIGINAL MIX)')).toBe('Glue (Original Mix)');
		expect(titleCase('FLY ME TO THE MOON')).toBe('Fly Me to the Moon');
	});

	it('keeps the short words whose capitals are the word', () => {
		expect(titleCase('HARDCORE EDM MIX')).toBe('Hardcore EDM Mix');
		expect(titleCase('SOMETHING (VIP)')).toBe('Something (VIP)');
		expect(titleCase('DJ SOMEBODY')).toBe('DJ Somebody');
		expect(titleCase('PART III')).toBe('Part III');
	});

	// One lower-case letter anywhere and the field is not shouted, so rule 1
	// has something to preserve again and takes over.
	it('is not shouted if one letter is not a capital', () => {
		expect(titleCase('HARDCORE EDM mix')).toBe('HARDCORE EDM Mix');
	});

	it('takes a field with no letters at all as it finds it', () => {
		expect(titleCase('808')).toBe('808');
	});
});

// The switch is the popup's own, under the key the sheet and the Manager panel
// both read it by — and off until it is asked for, since what the uploader
// typed is what the page shows.
describe('the switch', () => {
	beforeEach(() => {
		localStorage.clear();
		titleCasing.reload();
	});

	it('is off until somebody turns it on', () => {
		expect(titleCasing.value).toBe(false);
		expect(localStorage.getItem('pref:title-case')).toBe(null);
	});

	it('is remembered in the popup’s own storage', () => {
		titleCasing.value = true;
		expect(localStorage.getItem('pref:title-case')).toBe('1');
		titleCasing.value = false;
		expect(localStorage.getItem('pref:title-case')).toBe('0');
	});

	it('comes back as it was left', () => {
		localStorage.setItem('pref:title-case', '1');
		titleCasing.reload();
		expect(titleCasing.value).toBe(true);
	});
});
