import { expect, it } from 'vitest';
import { eventsOf } from './home-events';
import type { ShowView } from './types';

// SAFETY: eventsOf reads only a show's URI and its event.
const show = (uri: string, event: string) => ({ uri, record: { event: { uri: event, cid: 'c' } } }) as ShowView;

it('groups shows on the same calendar event into one lineup, in the order the events first come', () => {
	const shows = [show('a', 'friday'), show('b', 'saturday'), show('c', 'friday')];

	expect(eventsOf(shows).map((event) => [event.uri, event.shows.map((one) => one.uri)])).toEqual([
		['friday', ['a', 'c']],
		['saturday', ['b']]
	]);
	expect(eventsOf([])).toEqual([]);
});
