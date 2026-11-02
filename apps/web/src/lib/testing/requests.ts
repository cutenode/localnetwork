import type { AccountView, ProjectView, RequestKind, RequestView, Viewer } from '#lib/types.ts';

/** Made-up accounts, a project and its requests, for the request and staff pages' tests. */

export const ada: Viewer = {
	did: 'did:plc:ada',
	handle: 'ada.test',
	roles: ['admin'],
	isAdmin: true,
	isStaff: true,
	spacesCapable: true
};

export const neon: AccountView = { did: 'did:plc:neon', handle: 'neon.test' };

export const harbour: AccountView = { did: 'did:plc:harbour', handle: 'harbour.test' };

export const neonTide: ProjectView = {
	uri: 'at://did:plc:root/computer.localnetwork.project/2',
	rkey: '2',
	cid: 'c',
	slug: 'neon-tide',
	profile: { name: 'Neon Tide', kind: 'solo', genres: ['techno'], location: { city: 'Berlin', country: 'DE' } },
	managers: [neon.did],
	status: 'active',
	revision: 2,
	createdBy: neon.did,
	createdAt: '2026-01-01T00:00:00Z',
	updatedAt: '2026-01-01T00:00:00Z',
	avatarUrl: null
};

/** A request about Neon Tide: a tagline change from neon.test unless overridden. */
export const request = (kind: RequestKind, overrides: Partial<RequestView> = {}): RequestView => ({
	uri: `at://did:plc:root/space/computer.localnetwork.space.moderation/main/${neon.did}/computer.localnetwork.moderation.request/3k${kind}`,
	cid: 'c',
	rkey: `3k${kind}`,
	kind,
	status: 'pending',
	requester: neon,
	repoDid: neon.did,
	projectUri: neonTide.uri,
	project: { slug: neonTide.slug, name: neonTide.profile.name },
	record: {
		kind,
		baseRevision: 1,
		profile: { ...neonTide.profile, tagline: 'Dub' },
		createdAt: '2026-10-01T00:00:00Z'
	},
	createdAt: '2026-10-01T00:00:00Z',
	updatedAt: '2026-10-01T00:00:00Z',
	decision: null,
	resultProjectSlug: null,
	...overrides
});
