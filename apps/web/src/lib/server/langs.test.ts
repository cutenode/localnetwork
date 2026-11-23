import { describe, expect, it } from 'vitest';
import { editedLangs, withLangs } from './langs';

describe('withLangs', () => {
	it('names the languages of a record with prose, and none of one whose text is blank or missing', () => {
		expect(withLangs(['Doors at eleven', undefined], ['en'])).toEqual({ langs: ['en'] });
		expect(withLangs([' \n', undefined], ['en'])).toEqual({});
		expect(withLangs(['Doors at eleven'], undefined)).toEqual({});
	});
});

describe('editedLangs', () => {
	it("keeps the languages prose had while it's the same, and takes the writer's once it changes", () => {
		const before = { text: ['Techno am Sonntag'], langs: ['de'] };

		expect(editedLangs(['Techno am Sonntag'], before, ['en'])).toEqual({ langs: ['de'] });
		expect(editedLangs(['Techno on Sundays'], before, ['en'])).toEqual({ langs: ['en'] });
		expect(editedLangs(['Techno on Sundays'], null, ['en'])).toEqual({ langs: ['en'] });
	});
});
