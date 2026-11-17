import { DEMO_DJ_PROFILE, DEMO_DJ_SETS } from '../../../scripts/seed-data.ts';
import type { DjActivity } from '#lib/dj.ts';
import { djProfileSchema, djSetSchema } from '#lib/dj-schema.ts';
import type { ProfileRecord, ProfileView, ProjectView } from '#lib/types.ts';

/** Neon, the seed data's DJ, whose profile the tests and stories show. */
export const NEON = 'did:plc:neon';

/** Neon's profile with `record` over a bare one named Neon Tide. */
export const neonProfile = (record: Partial<ProfileRecord>): ProfileView => ({
	did: NEON,
	uri: `at://${NEON}/computer.localnetwork.profile/self`,
	cid: 'bafyprofile',
	record: {
		basics: { name: 'Neon Tide' },
		createdAt: '2026-10-30T00:00:00Z',
		updatedAt: '2026-10-30T00:00:00Z',
		...record
	},
	avatarUrl: null
});

/** An active project Neon manages. */
export const NEON_CREW: ProjectView = {
	uri: 'at://did:plc:root/computer.localnetwork.project/1',
	rkey: '1',
	cid: 'bafyproject',
	slug: 'neon-crew',
	profile: { name: 'Neon Crew', kind: 'duo', genres: ['techno'], location: { city: 'Berlin', country: 'DE' } },
	managers: [NEON],
	status: 'active',
	revision: 1,
	createdBy: 'did:plc:root',
	createdAt: '2026-01-01T00:00:00Z',
	updatedAt: '2026-01-01T00:00:00Z',
	avatarUrl: null
};

/** Neon's records as locallibrary publishes them: their sets, and their playing stats with `stats`. */
export const neonPlaying = ({ stats = false } = {}): DjActivity[] => [
	{
		did: NEON,
		handle: 'neon.test',
		profile: stats ? djProfileSchema.parse(DEMO_DJ_PROFILE) : null,
		sets: DEMO_DJ_SETS.map((set, index) => ({
			...djSetSchema.parse(set),
			uri: `at://${NEON}/computer.localnetwork.dj.set/${index}`
		}))
	}
];
