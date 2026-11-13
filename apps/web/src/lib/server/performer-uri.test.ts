import { describe, expect, it } from 'vitest';
import { COLLECTION } from '#lib/nsid.ts';
import { parsePerformerUri, performerUri } from './performer-uri.ts';
import type { PerformerRef } from '#lib/types.ts';

const ROOT = 'did:plc:root';

describe('performer URIs', () => {
	const refs: [PerformerRef, string][] = [
		[{ kind: 'dj', did: 'did:plc:ada' }, `at://did:plc:ada/${COLLECTION.profile}/self`],
		[{ kind: 'act', skey: '3kact' }, `at://${ROOT}/${COLLECTION.listing}/3kact`],
		[{ kind: 'project', rkey: '3kproject' }, `at://${ROOT}/${COLLECTION.project}/3kproject`]
	];

	it.each(refs)('names %o by %s, and reads it back', (ref, uri) => {
		expect(performerUri(ROOT, ref)).toBe(uri);
		expect(parsePerformerUri(ROOT, uri)).toEqual(ref);
	});

	it("names nothing with another account's act or project, a profile under another key, or any other record", () => {
		for (const uri of [
			`at://did:plc:ada/${COLLECTION.listing}/3kact`,
			`at://did:plc:ada/${COLLECTION.project}/3kproject`,
			`at://did:plc:ada/${COLLECTION.profile}/other`,
			`at://handle.test/${COLLECTION.profile}/self`,
			`at://${ROOT}/${COLLECTION.show}/3kshow`,
			`at://${ROOT}/space/${COLLECTION.listing}/3kact`,
			'not a uri'
		])
			expect(parsePerformerUri(ROOT, uri), uri).toBeNull();
	});
});
