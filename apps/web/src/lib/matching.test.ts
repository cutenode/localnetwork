import { describe, expect, it } from 'vitest';
import { noteTexts } from './matching';

describe('noteTexts', () => {
	it('words each note the way the matcher always has', () => {
		expect(
			noteTexts([
				{ code: 'plays', genres: ['techno', 'electro', 'acid'] },
				{ code: 'adjacentSound' },
				{ code: 'noGenreOverlap' },
				{ code: 'locationUnknown' },
				{ code: 'local' },
				{ code: 'fitsBudget' },
				{ code: 'aboveBudget' },
				{ code: 'wellAboveBudget' },
				{ code: 'fitsRoom' },
				{ code: 'biggerRoom' },
				{ code: 'smallerRoom' },
				{ code: 'setType', setType: 'allNight' },
				{ code: 'formats', formats: ['vinyl', 'cdj'] },
				{ code: 'differentSetup' }
			])
		).toEqual([
			'Plays techno, electro, acid',
			'Adjacent sound',
			"Genres don't overlap",
			'Location unknown',
			'Local',
			'Fits budget',
			'Usually above this budget',
			'Well above this budget',
			'Plays rooms this size',
			'Bigger room than usual',
			'Smaller room than usual',
			'Does all night long sets',
			'Plays Vinyl / CDJs / USB',
			'Different setup'
		]);
	});

	it('turns meters into the distances people read', () => {
		expect(
			noteTexts([
				{ code: 'nearby', distanceM: 700 },
				{ code: 'nearby', distanceM: 3_240 },
				{ code: 'travels', distanceM: 504_400 },
				{ code: 'outOfRange', distanceM: 1_234_567 }
			])
		).toEqual(['under 1 km away', '3.2 km away', 'Travels 504 km', '1,235 km away']);
	});

	it('leaves out codes this app does not know yet', () => {
		expect(noteTexts([{ code: 'vibes' }, { code: 'constructor' }, { code: 'local' }])).toEqual(['Local']);
	});
});
