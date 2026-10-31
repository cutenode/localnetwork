import type { JsonObject } from '@atproto/lex-json';
import type { LexiconDoc } from '@atproto/lexicon';
import { describe, expect, it } from 'vitest';
import { z } from 'zod';
import { COLLECTION } from '#lib/nsid.ts';
import { LEXICON_FILES } from '../../../../scripts/lib/lexicon-files.ts';
import { DEMO_DJ_SETS } from '../../../../scripts/seed-data.ts';
import { LEXICON_DOCS, RecordValidationError, REFERENCED_DOCS, validateRecord } from './lexicons.ts';

describe('the bundled lexicons', () => {
	// The scripts' loader reads lexicons/ from disk, so this also checks the two loaders agree.
	const onDisk = LEXICON_FILES.map((file) => z.object({ id: z.string() }).parse(file).id).toSorted();

	it("are every file in lexicons/, ours apart from other authorities'", () => {
		const ids = (docs: LexiconDoc[]) => docs.map((doc) => doc.id);

		expect([...ids(LEXICON_DOCS), ...ids(REFERENCED_DOCS)].toSorted()).toEqual(onDisk);
		expect(ids(LEXICON_DOCS).toSorted()).toEqual(onDisk.filter((id) => id.startsWith('computer.localnetwork.')));
		expect(ids(LEXICON_DOCS)).toContain('computer.localnetwork.locallibraryPublishing');
		expect(ids(REFERENCED_DOCS)).toEqual(
			expect.arrayContaining(['app.userinput.discussion', 'com.atproto.repo.strongRef'])
		);
	});
});

describe('LEXICON_DOCS', () => {
	it('keeps the refs as written, so published schemas match lexicons/', () => {
		const refs = JSON.stringify(LEXICON_DOCS).match(/"#[^"]*"|"lex:[^"]*"/g) ?? [];

		expect(refs.length).toBeGreaterThan(0);
		expect(refs.filter((ref) => ref.startsWith('"lex:'))).toEqual([]);
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
