import { describe, expect, it } from 'vitest';
import { formFields } from './form-fields.svelte.ts';

describe('formFields', () => {
	it('copies the values all the way down, leaving them alone', () => {
		const values = { title: 'Clubs', quotes: [{ text: 'Relentless', source: 'Mixmag' }] };
		const fields = formFields(values);

		fields.title = 'Festivals';
		fields.quotes[0].text = 'Patient';
		fields.quotes.push({ text: 'Warm', source: 'RA' });

		expect(values).toEqual({ title: 'Clubs', quotes: [{ text: 'Relentless', source: 'Mixmag' }] });
		expect(fields.quotes.map((quote) => quote.text)).toEqual(['Patient', 'Warm']);
	});

	it('copies a copy too, so binding never writes through to the state it came from', () => {
		const draft = formFields({ title: 'Clubs' });
		const fields = formFields(draft);

		fields.title = 'Festivals';

		expect(draft.title).toBe('Clubs');
	});
});
