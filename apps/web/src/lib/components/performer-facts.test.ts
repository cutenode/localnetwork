import { render } from 'svelte/server';
import { describe, expect, it } from 'vitest';
import PerformerFacts from './PerformerFacts.svelte';

// Public profiles and project pages render the facts on the server, so they're there before any script runs.
describe('PerformerFacts on the server', () => {
	it('renders each fact on file, with its note', () => {
		const { body } = render(PerformerFacts, {
			props: {
				performer: {
					formats: ['vinyl'],
					setLength: { maxMinutes: 240 },
					capacity: { min: 50, max: 400 },
					travel: { radiusKm: 0, notes: 'Flights from BER' }
				}
			}
		});

		expect(textOf(body)).toEqual([
			'Plays on',
			'Vinyl',
			'Set length',
			'?–240 minutes',
			'Rooms',
			'50–400 people',
			'Travel',
			'Local only',
			'Flights from BER'
		]);
	});

	it('renders nothing without facts on file', () => {
		const { body } = render(PerformerFacts, { props: { performer: {} } });

		expect(textOf(body)).toEqual([]);
	});
});

// Helpers

/** The text of each element in server-rendered HTML, in order. */
function textOf(html: string) {
	return html
		.split(/<[^>]*>/)
		.map((text) => text.trim())
		.filter(Boolean);
}
