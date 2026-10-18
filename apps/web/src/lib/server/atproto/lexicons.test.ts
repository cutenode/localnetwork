import type { JsonObject } from '@atproto/lex-json';
import { describe, expect, it, vi } from 'vitest';
import { COLLECTION } from '#lib/nsid.ts';
import { DEMO_DJ_SETS } from '../../../../scripts/seed-data.ts';
import { standInLexicons } from '../testing/lexicons.ts';
import { LEXICON_DOCS, RecordValidationError, validateRecord } from './lexicons.ts';

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

describe('loadReferencedLexicons', () => {
	/** lexicons.ts as a process that hasn't loaded them yet has it. */
	const fresh = async () => {
		vi.resetModules();

		return import('./lexicons.ts');
	};

	const venue = { ...DEMO_DJ_SETS[0], createdAt: '2026-10-17T12:00:00.000Z' };

	it('fetches each one once, after which records that use them validate', async () => {
		const loader = await fresh();
		const asked: string[] = [];

		const source = async (nsid: string) => {
			asked.push(nsid);

			return standInLexicons(nsid);
		};

		expect(venue.location).toMatchObject({ $type: 'community.lexicon.location.address' });
		expect(() => loader.referencedLexicons()).toThrow('not loaded yet');
		expect(() => loader.validateRecord(COLLECTION.djSet, venue)).toThrow('community.lexicon.location.address');

		const docs = await loader.loadReferencedLexicons(source);
		await loader.loadReferencedLexicons(source);

		expect(asked).toEqual(loader.REFERENCED_NSIDS);
		expect(docs.map((doc) => doc.id)).toEqual([...loader.REFERENCED_NSIDS, 'com.atproto.repo.strongRef']);
		expect(loader.referencedLexicons()).toBe(docs);
		expect(loader.validateRecord(COLLECTION.djSet, venue)).toMatchObject({ $type: COLLECTION.djSet });
	});

	it('tries again after a fetch fails', async () => {
		const loader = await fresh();

		await expect(loader.loadReferencedLexicons(async () => Promise.reject(new Error('offline')))).rejects.toThrow(
			'offline'
		);
		expect(() => loader.referencedLexicons()).toThrow('not loaded yet');
		await expect(loader.loadReferencedLexicons(standInLexicons)).resolves.toHaveLength(2);
	});
});
