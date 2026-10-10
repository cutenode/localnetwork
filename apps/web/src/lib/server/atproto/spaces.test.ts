import { describe, expect, it } from 'vitest';
import {
	bookingSpace,
	isSpaceType,
	parsePublicRecordUri,
	parseSpaceRecordUri,
	parseSpaceUri,
	plannerSpace,
	publicRecordUri,
	rootSpaces,
	spaceOfRecord,
	spaceRecordUri,
	spaceUri
} from './spaces';

const ROOT = 'did:plc:root123';

describe('space URIs', () => {
	it('builds and parses space refs', () => {
		const uri = spaceUri(ROOT, 'computer.localnetwork.space.audit', 'main');
		expect(uri).toBe('at://did:plc:root123/space/computer.localnetwork.space.audit/main');
		expect(parseSpaceUri(uri)).toEqual({ authority: ROOT, type: 'computer.localnetwork.space.audit', skey: 'main' });
		expect(parseSpaceUri('at://did:plc:x/app.bsky.feed.post/1')).toBeNull();
	});

	it('builds and parses space record URIs', () => {
		const space = rootSpaces(ROOT).moderation;
		const uri = spaceRecordUri(space, 'did:plc:alice', 'computer.localnetwork.moderation.request', '3abc');
		expect(parseSpaceRecordUri(uri)).toMatchObject({ authority: ROOT, author: 'did:plc:alice', rkey: '3abc' });
		expect(spaceOfRecord(uri)).toBe(space);
		expect(spaceOfRecord('nope')).toBeNull();
	});

	it('handles public record URIs and refuses space URIs there', () => {
		const uri = publicRecordUri(ROOT, 'computer.localnetwork.project', 'abc');
		expect(parsePublicRecordUri(uri)).toEqual({ did: ROOT, collection: 'computer.localnetwork.project', rkey: 'abc' });
		expect(parsePublicRecordUri(`at://${ROOT}/space/x`)).toBeNull();
	});

	it('names root, booking, and planner spaces', () => {
		expect(Object.keys(rootSpaces(ROOT))).toEqual(['audit', 'moderation', 'staff', 'industry']);
		expect(bookingSpace(ROOT, 'tid1')).toBe(`at://${ROOT}/space/computer.localnetwork.space.booking/tid1`);
		expect(plannerSpace('did:plc:pat')).toBe('at://did:plc:pat/space/computer.localnetwork.space.planner/self');
		expect(isSpaceType('computer.localnetwork.space.booking')).toBe(true);
		expect(isSpaceType('app.bsky.feed.post')).toBe(false);
	});
});
