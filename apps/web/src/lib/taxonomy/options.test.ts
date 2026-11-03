import { describe, expect, it } from 'vitest';
import djSet from '../../../../../lexicons/computer/localnetwork/dj/set.json' with { type: 'json' };
import { labelFor, SET_KINDS } from './options';

describe('SET_KINDS', () => {
	it('labels every kind the set lexicon knows', () => {
		expect(SET_KINDS.map((kind) => kind.value)).toEqual(djSet.defs.main.record.properties.kind.knownValues);
	});

	it("reads mix as Studio, and a DJ's own word as they wrote it", () => {
		expect(labelFor(SET_KINDS, 'mix')).toBe('Studio');
		expect(labelFor(SET_KINDS, 'studio')).toBe('Studio');
		expect(labelFor(SET_KINDS, 'warehouse rave')).toBe('warehouse rave');
	});
});
