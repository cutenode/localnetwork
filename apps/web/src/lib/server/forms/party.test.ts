import { describe, expect, it } from 'vitest';
import { partyValues } from './party';

describe('partyValues', () => {
	it("gives a party's page as its form's text, empty for what it hasn't got", () => {
		expect(partyValues({ name: 'Night Shift' })).toEqual({
			name: 'Night Shift',
			bio: '',
			city: '',
			region: '',
			country: ''
		});
		expect(
			partyValues({ name: 'Night Shift', bio: 'Techno', location: { city: 'Berlin', region: 'Berlin', country: 'DE' } })
		).toEqual({ name: 'Night Shift', bio: 'Techno', city: 'Berlin', region: 'Berlin', country: 'DE' });
	});
});
