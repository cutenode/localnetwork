import { expect, it } from 'vitest';
import type { ProfileView } from '#lib/types.ts';
import { profileDid, takesInquiries } from './booking-performers';

it("reads the DJ out of a profile's URI, and nothing out of any other", () => {
	expect(profileDid('at://did:plc:neon/computer.localnetwork.profile/self')).toBe('did:plc:neon');
	expect(profileDid('at://did:plc:root/computer.localnetwork.project/2')).toBeNull();
	expect(profileDid('at://did:plc:neon/computer.localnetwork.profile/other')).toBeNull();
	expect(profileDid('at://neon.test/computer.localnetwork.profile/self')).toBeNull();
	expect(profileDid('not a uri')).toBeNull();
});

it('takes inquiries only for a listed DJ with the performer section on who turned on Take inquiries', () => {
	const listing = {
		uri: 'at://did:plc:root/computer.localnetwork.listing.profile/did:plc:neon',
		cid: 'bafy',
		createdAt: ''
	};

	const dj = (record: Partial<ProfileView['record']>): ProfileView => ({
		did: 'did:plc:neon',
		uri: 'at://did:plc:neon/computer.localnetwork.profile/self',
		cid: 'bafy',
		record: { basics: { name: 'Neon' }, createdAt: '', updatedAt: '', ...record },
		avatarUrl: null
	});

	expect(takesInquiries(listing, dj({ performer: {}, takesInquiries: true }))).toBe(true);
	expect(takesInquiries(listing, dj({ performer: {} }))).toBe(false);
	expect(takesInquiries(listing, dj({ performer: {}, takesInquiries: false }))).toBe(false);
	expect(takesInquiries(listing, dj({ takesInquiries: true }))).toBe(false);
	expect(takesInquiries(null, dj({ performer: {}, takesInquiries: true }))).toBe(false);
	expect(takesInquiries(listing, null)).toBe(false);
});
