import type { BookingThreadView, PlannerShowView, Viewer } from '#lib/types.ts';
import type { PageData } from './$types';

export const PAT: Viewer = {
	did: 'did:plc:pat',
	handle: 'pat.test',
	roles: [],
	isAdmin: false,
	isStaff: false,
	spacesCapable: true
};

const thread = (
	skey: string,
	title: string,
	project: string,
	status: string,
	showDate: string | null = null
): BookingThreadView => ({
	space: `at://did:plc:root/computer.localnetwork.space.booking/${skey}`,
	skey,
	status,
	title,
	showDate,
	project: {
		uri: `at://did:plc:root/computer.localnetwork.project/${skey}`,
		slug: project.toLowerCase().replaceAll(' ', '-'),
		name: project
	},
	promoter: { did: PAT.did, handle: PAT.handle },
	participants: [{ did: PAT.did, handle: PAT.handle, role: 'promoter' }],
	lastAuthor: PAT.did,
	lastActivityAt: '2026-11-10T12:00:00Z',
	createdAt: '2026-11-01T12:00:00Z'
});

export const INQUIRIES = [
	thread('3k1', 'Warehouse closing set', 'Halflight', 'negotiating', '2027-02-14T23:00:00.000Z'),
	thread('3k2', 'Rooftop opener', 'Neon Tide', 'confirmed', '2027-03-06T20:00:00.000Z'),
	thread('3k3', 'Basement residency', 'Okon', 'open')
];

const show = (rkey: string, title: string, lineup: number, date?: string, venue?: string): PlannerShowView => ({
	rkey,
	details: { title, date, venue },
	lineup: Array.from({ length: lineup }, (_, index) => ({
		project: `at://did:plc:root/computer.localnetwork.project/${index}`
	})),
	createdAt: '2026-11-01T12:00:00Z'
});

export const SHOWS = [
	show('3s1', 'Warehouse', 3, '2027-02-14T22:00:00.000Z', 'The Dock'),
	show('3s2', 'Winter Social', 0)
];

/** The page's data for Pat, a promoter, with `overrides` on top. */
export const promoterPage = (overrides: Partial<PageData> = {}): PageData => ({
	viewer: PAT,
	pendingRequests: 0,
	unread: 0,
	waiting: 0,
	needs: [],
	access: Promise.resolve([]),
	projects: Promise.resolve([]),
	sections: Promise.resolve({ performer: false, promoter: true }),
	geocoder: false,
	inquiries: INQUIRIES,
	inquiryCount: INQUIRIES.length,
	planner: Promise.resolve({ available: true, shows: SHOWS, total: SHOWS.length }),
	...overrides
});
