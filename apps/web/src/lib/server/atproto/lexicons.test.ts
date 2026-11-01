import type { JsonObject } from '@atproto/lex-json';
import type { LexiconDoc } from '@atproto/lexicon';
import { describe, expect, it } from 'vitest';
import { z } from 'zod';
import { COLLECTION, PERMISSIONS } from '#lib/nsid.ts';
import { LEXICON_FILES } from '../../../../scripts/lib/lexicon-files.ts';
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

	describe('profiles', () => {
		const at = '2026-10-30T00:00:00.000Z';
		const avatar = { $type: 'blob', ref: { $link: cid }, mimeType: 'image/webp', size: 100 };

		const profile = (sections: JsonObject) => ({
			basics: { name: 'Neon Tide' },
			...sections,
			createdAt: at,
			updatedAt: at
		});

		it('turns a section on with an empty object', () => {
			expect(validateRecord(COLLECTION.profile, profile({ performer: {} }))).toMatchObject({ performer: {} });
			expect(validateRecord(COLLECTION.profile, profile({ promoter: {} }))).toMatchObject({ promoter: {} });
			expect(validateRecord(COLLECTION.profile, profile({}))).not.toHaveProperty('performer');
		});

		it('holds every section filled in', () => {
			const record = profile({
				basics: {
					name: 'Neon Tide',
					avatar,
					bio: 'Warehouse techno.',
					pronouns: 'they/them',
					location: { city: 'Berlin', country: 'DE' },
					links: [{ kind: 'soundcloud', url: 'https://soundcloud.com/neontide' }]
				},
				performer: {
					tagline: 'Four hours of rolling techno.',
					genres: ['techno'],
					formats: ['vinyl'],
					setTypes: ['closing'],
					setLength: { minMinutes: 60, maxMinutes: 240 },
					capacity: { min: 100, max: 800 },
					fees: [{ tier: 'emerging', currency: 'EUR', minAmount: 300 }],
					travel: { radiusKm: 500, international: true },
					mixes: [{ title: 'Boiler set', url: 'https://example.com/mix' }],
					highlights: ['Tresor'],
					lookingFor: 'Late slots.',
					activeSince: 2016
				},
				promoter: { cities: [{ city: 'Leipzig', country: 'DE' }] }
			});

			expect(validateRecord(COLLECTION.profile, record)).toMatchObject({ $type: COLLECTION.profile });
		});

		it('needs basics with a short name', () => {
			expect(() => validateRecord(COLLECTION.profile, profile({ basics: {} }))).toThrow(RecordValidationError);
			expect(() => validateRecord(COLLECTION.profile, profile({ basics: { name: 'n'.repeat(65) } }))).toThrow(
				RecordValidationError
			);
			expect(() => validateRecord(COLLECTION.profile, { createdAt: at, updatedAt: at })).toThrow(RecordValidationError);
		});

		it('lists a DJ by their DID, and asks staff to', () => {
			const did = 'did:plc:neon';
			const request = 'at://did:plc:neon/computer.localnetwork.moderation.request/3mreq';

			expect(validateRecord(COLLECTION.listingProfile, { subject: did, request, createdAt: at })).toMatchObject({
				subject: did
			});
			expect(() => validateRecord(COLLECTION.listingProfile, { subject: 'neon', createdAt: at })).toThrow(
				RecordValidationError
			);
			expect(
				validateRecord(COLLECTION.request, {
					kind: 'list',
					listing: { subject: `at://${did}/${COLLECTION.profile}/self`, cid },
					createdAt: at
				})
			).toMatchObject({ kind: 'list' });
			expect(() => validateRecord(COLLECTION.request, { kind: 'list', listing: { cid }, createdAt: at })).toThrow(
				RecordValidationError
			);
		});
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
