import type { MigrationPlan, ProjectPlan } from '#lib/project-migration.ts';

const ROOT = 'did:plc:root';

/** One project's plan: an act with a member and a manager, nothing blocking, unless `overrides` say otherwise. */
export const projectPlan = (slug: string, name: string, overrides: Partial<ProjectPlan> = {}): ProjectPlan => ({
	uri: `at://${ROOT}/computer.localnetwork.project/3k${slug}`,
	slug,
	name,
	target: 'act',
	dj: null,
	space: `ats://${ROOT}/computer.localnetwork.space.performer/3k${slug}`,
	performerUri: `at://${ROOT}/computer.localnetwork.listing/3k${slug}`,
	record: null,
	people: [
		{ did: 'did:plc:saffron', handle: 'saffron.test', role: 'member', relationship: 'member' },
		{ did: 'did:plc:agent', handle: 'agent.test', role: 'manager', relationship: 'agent' }
	],
	listed: true,
	hasDetails: true,
	drafts: [],
	pressKits: [{ uri: `at://${ROOT}/computer.localnetwork.epk/3kkit`, title: 'Clubs' }],
	bookingThreads: [{ space: 'ats://booking', title: 'Rooftop', status: 'open' }],
	lazy: { follows: 2, upcomingShows: 1 },
	decline: [],
	redirect: { from: `/projects/${slug}`, to: `/act/${slug}` },
	unmapped: [],
	blockers: [],
	...overrides
});

/** The plan for the seed's mix: an act, a DJ found by their claim, and a blocked DJ. */
export const migrationPlan: MigrationPlan = {
	projects: [
		projectPlan('saffron-social', 'Saffron Social'),
		projectPlan('neon-tide', 'Neon Tide', {
			target: 'dj',
			dj: { did: 'did:plc:neon', handle: 'neon.test', rule: 'self' },
			people: [{ did: 'did:plc:neon', handle: 'neon.test', role: 'member', relationship: 'self' }],
			redirect: { from: '/projects/neon-tide', to: '/@neon.test' },
			unmapped: [{ field: 'tagline', reason: "Not copied: the DJ's own profile is their page" }]
		}),
		projectPlan('moss-and-marrow', 'Moss & Marrow', {
			listed: false,
			hasDetails: false,
			people: [],
			lazy: { follows: 0, upcomingShows: 0 },
			unmapped: [{ field: 'kind', reason: 'An act has no "live" form, so it\'s planned as a group' }],
			blockers: [{ kind: 'noManagers' }]
		}),
		projectPlan('pirate-signal', 'Pirate Signal', {
			target: 'dj',
			dj: { did: null, handle: null, rule: 'unresolved' },
			performerUri: null,
			people: [{ did: 'did:plc:pirate', handle: 'pirate.test', role: 'manager', relationship: null }],
			decline: [
				{ uri: 'ats://request', kind: 'claim', requester: 'did:plc:stranger', createdAt: '2026-11-01T00:00:00Z' }
			],
			redirect: { from: '/projects/pirate-signal', to: null },
			blockers: [
				{ kind: 'unresolvedDj' },
				{ kind: 'legacyPressKit', uri: 'at://did:plc:pirate/computer.localnetwork.epk/3kold', did: 'did:plc:pirate' }
			]
		})
	],
	pendingCreates: [
		{ uri: 'ats://create', kind: 'create', requester: 'did:plc:new', createdAt: '2026-11-01T00:00:00Z' }
	],
	totals: { acts: 2, djs: 2, blocked: 2 }
};
