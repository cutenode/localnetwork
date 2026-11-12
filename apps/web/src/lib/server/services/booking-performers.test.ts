import { expect, it } from 'vitest';
import { profileDid } from './booking-performers';

it("reads the DJ out of a profile's URI, and nothing out of any other", () => {
	expect(profileDid('at://did:plc:neon/computer.localnetwork.profile/self')).toBe('did:plc:neon');
	expect(profileDid('at://did:plc:root/computer.localnetwork.project/2')).toBeNull();
	expect(profileDid('at://did:plc:neon/computer.localnetwork.profile/other')).toBeNull();
	expect(profileDid('at://neon.test/computer.localnetwork.profile/self')).toBeNull();
	expect(profileDid('not a uri')).toBeNull();
});
