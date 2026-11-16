import { isLexValue } from '@atproto/lex-data';
import { isValidRecordKey } from '@atproto/syntax';
import * as fc from 'fast-check';
import { describe, expect, it } from 'vitest';
import { RecordValidationError, validateRecord } from '../atproto/lexicons';
import { anyRecord, nearlyValidRecord, RECORD_COLLECTIONS, recordKey, validRecord } from './lexicon-arbitraries';

describe('lexicon arbitraries', () => {
	it('cover every record collection', () => {
		expect(RECORD_COLLECTIONS).toContain('computer.localnetwork.dj.set');
		expect(RECORD_COLLECTIONS).toContain('app.bsky.actor.profile');
		expect(RECORD_COLLECTIONS).toContain('community.lexicon.calendar.event');
	});

	describe.each(RECORD_COLLECTIONS)('%s', (collection) => {
		it('makes records validateRecord accepts', () => {
			fc.assert(
				fc.property(validRecord(collection), (record) => {
					expect(validateRecord(collection, record)).toEqual(record);
				}),
				{ numRuns: 40 }
			);
		});

		it('makes nearly valid records validateRecord refuses', () => {
			fc.assert(
				fc.property(nearlyValidRecord(collection), ({ record, path }) => {
					expect(() => validateRecord(collection, record), `broken at ${path}`).toThrow(RecordValidationError);
				}),
				{ numRuns: 40 }
			);
		});

		it('types any record as the collection, holding only data a repo can', () => {
			fc.assert(
				fc.property(anyRecord(collection), (record) => {
					expect(record.$type).toBe(collection);
					expect(isLexValue(record)).toBe(true);
				}),
				{ numRuns: 20 }
			);
		});

		it('makes record keys the lexicon allows', () => {
			fc.assert(
				fc.property(recordKey(collection), (rkey) => {
					expect(isValidRecordKey(rkey)).toBe(true);
				}),
				{ numRuns: 20 }
			);
		});
	});
});
