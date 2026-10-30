import type { JsonObject } from '@atproto/lex-json';
import type { LexiconDoc } from '@atproto/lexicon';
import { describe, expect, it } from 'vitest';
import { COLLECTION, PERMISSIONS } from '#lib/nsid.ts';
import { DEMO_DJ_SETS } from '../../../../scripts/seed-data.ts';
import {
	LEXICON_DOCS,
	permissionSetFor,
	RecordValidationError,
	REFERENCED_DOCS,
	ROOT_AUTHORITY,
	schemaRecord,
	validateRecord
} from './lexicons.ts';

describe('LEXICON_DOCS', () => {
	it('keeps the refs as written, so published schemas match lexicons/', () => {
		const refs = JSON.stringify(LEXICON_DOCS).match(/"#[^"]*"|"lex:[^"]*"/g) ?? [];

		expect(refs.length).toBeGreaterThan(0);
		expect(refs.filter((ref) => ref.startsWith('"lex:'))).toEqual([]);
	});
});

describe('schemaRecord', () => {
	const root = 'did:plc:root';
	const authorities = (doc: LexiconDoc | JsonObject) => JSON.stringify(doc).match(/"authority":"[^"]*"/g) ?? [];

	it("publishes the permission set with the root's DID for every shared space, and no open authority", () => {
		const doc = LEXICON_DOCS.find((lexicon) => lexicon.id === PERMISSIONS)!;
		const record = schemaRecord(doc, root);

		expect(authorities(doc)).toContain(`"authority":"${ROOT_AUTHORITY}"`);
		expect(authorities(record).length).toBe(authorities(doc).length);
		expect(new Set(authorities(record))).toEqual(new Set([`"authority":"${root}"`]));
		expect(record).toEqual({
			$type: 'com.atproto.lexicon.schema',
			...doc,
			defs: { ...doc.defs, main: permissionSetFor(root) }
		});
	});

	it('publishes every other lexicon as written', () => {
		for (const doc of LEXICON_DOCS.filter((lexicon) => lexicon.id !== PERMISSIONS))
			expect(schemaRecord(doc, root)).toEqual({ $type: 'com.atproto.lexicon.schema', ...doc });
	});
});

describe('validateRecord', () => {
	const cid = 'bafkreie5737gdxlw5i64vzichcalba3z2v5n6icifvx5xytvske7mr3hpm';

	const project = (avatar: JsonObject) => ({
		slug: 'neon-tide',
		profile: {
			name: 'Neon Tide',
			kind: 'solo',
			genres: ['techno'],
			location: { city: 'Berlin', country: 'DE' },
			avatar
		},
		managers: ['did:plc:neon'],
		status: 'active',
		revision: 1,
		createdBy: 'did:plc:root',
		createdAt: '2026-10-06T00:00:00.000Z',
		updatedAt: '2026-10-06T00:00:00.000Z'
	});

	it('accepts typed blob refs', () => {
		const avatar = { $type: 'blob', ref: { $link: cid }, mimeType: 'image/png', size: 100 };

		expect(validateRecord(COLLECTION.project, project(avatar)).profile.avatar).toEqual(avatar);
	});

	it('rejects legacy blob refs, which the app types never model', () => {
		expect(() => validateRecord(COLLECTION.project, project({ cid, mimeType: 'image/png' }))).toThrow(
			RecordValidationError
		);
	});
});

describe('REFERENCED_DOCS', () => {
	it("holds every other authority's lexicon ours refer to", () => {
		const ids = new Set<string>(REFERENCED_DOCS.map((doc) => doc.id));
		const refs = [...JSON.stringify(LEXICON_DOCS).matchAll(/"((?:com|community)\.[\w.]+?)(?:#\w+)?"/g)];

		expect(refs.length).toBeGreaterThan(0);
		expect(refs.flatMap(([, nsid]) => (ids.has(nsid) ? [] : [nsid]))).toEqual([]);
	});

	it('validates the records that use them as their authorities define them', () => {
		const set = { ...DEMO_DJ_SETS[0], createdAt: '2026-10-17T12:00:00.000Z' };
		const noCountry = { $type: 'community.lexicon.location.address', locality: 'Berlin' };

		expect(set.location).toMatchObject({ $type: 'community.lexicon.location.address' });
		expect(validateRecord(COLLECTION.djSet, set)).toMatchObject({ $type: COLLECTION.djSet });
		expect(() => validateRecord(COLLECTION.djSet, { ...set, location: noCountry })).toThrow(
			'location must have the property "country"'
		);
	});
});
