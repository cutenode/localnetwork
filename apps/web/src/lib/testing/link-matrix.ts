import { DEMO_DJ_PROFILE, DEMO_DJ_SETS } from '../../../scripts/seed-data.ts';
import { djProfileSchema, djSetSchema } from '#lib/dj-schema.ts';
import type { ProjectView, Viewer } from '#lib/types.ts';

/** A row of docs/link-matrix.md: the page type it's listed under, what it links to, and where main stands. */
export type MatrixRow = { pageType: string; linksTo: string; onMain: string };

/**
 * A link a visitor finds by its accessible name. `within` names the region it's in, and `reveal` is text in that
 * region to click first, for links folded away until opened.
 */
export type ExpectedLink = { name: string | RegExp; href: string; within?: string; reveal?: string };

/**
 * One page type's checks: `render` shows the page for a row (its Links to text) with data where the row's When holds,
 * and `links` are the links it must have, keyed by the Links to text of the rows marked `on main`.
 */
export type LinkChecks = {
	pageType: string;
	render: (row: string) => Promise<{ container: HTMLElement }>;
	links: Record<string, ExpectedLink[]>;
};

/** Every row of the matrix's tables that have Links to and On main columns, under the `##` heading above each. */
export function matrixRows(markdown: string): MatrixRow[] {
	const rows: MatrixRow[] = [];
	let pageType = '';
	let header: string[] | null = null;

	for (const raw of markdown.split('\n')) {
		const line = raw.trim();

		if (line.startsWith('## ')) pageType = line.slice(3);

		if (!line.startsWith('|')) {
			header = null;
			continue;
		}

		const cells = line
			.slice(1, -1)
			.split('|')
			.map((cell) => cell.trim());

		if (!header) {
			header = cells;
			continue;
		}

		const linksTo = header.indexOf('Links to');
		const onMain = header.indexOf('On main');
		const isDivider = cells.every((cell) => /^-+$/.test(cell));

		if (linksTo === -1 || onMain === -1 || isDivider) continue;
		rows.push({ pageType, linksTo: cells[linksTo], onMain: cells[onMain] });
	}

	return rows;
}

export const NEON = { did: 'did:plc:neon', handle: 'neon.test' };

/** A signed-in visitor who manages nothing on the page. */
export const PAT: Viewer = {
	did: 'did:plc:pat',
	handle: 'pat.test',
	roles: [],
	isAdmin: false,
	isStaff: false,
	spacesCapable: true
};

/** neon.test's act, with a mix that has a tracklist. */
export const NEON_TIDE: ProjectView = {
	uri: 'at://did:plc:root/computer.localnetwork.project/2',
	rkey: '2',
	cid: 'c',
	slug: 'neon-tide',
	profile: {
		name: 'Neon Tide',
		kind: 'solo',
		genres: ['techno'],
		location: { city: 'Berlin', country: 'DE' },
		mixes: [
			{
				title: 'Tidal Hours',
				url: 'https://soundcloud.com/neon/tidal-hours',
				tracklist: { $type: 'blob', ref: { $link: 'bafytracklist' }, mimeType: 'application/json', size: 2 }
			}
		]
	},
	managers: [NEON.did],
	status: 'active',
	revision: 1,
	createdBy: 'did:plc:root',
	createdAt: '2026-01-01T00:00:00Z',
	updatedAt: '2026-01-01T00:00:00Z',
	avatarUrl: null
};

/** neon.test's playing stats and published sets, from the demo network's seed. */
export const NEON_DJ = {
	...NEON,
	profile: djProfileSchema.parse(DEMO_DJ_PROFILE),
	sets: DEMO_DJ_SETS.map((set, i) => ({
		...djSetSchema.parse(set),
		uri: `at://${NEON.did}/computer.localnetwork.dj.set/${i}`
	}))
};

/** NEON_DJ's first set, under Recent sets, linking its page. */
export const SET_PAGE: ExpectedLink = { name: 'Tidal Hours', href: '/@neon.test/sets/0', within: 'Recent sets' };

/** The first artist in NEON_DJ's first set, in its tracklist under Recent sets. */
export const SET_ARTIST: ExpectedLink = {
	name: 'Ferrule',
	href: '/artists/2a8e4f60-1c3b-4d7e-9a05-6b1f0c2d3e41',
	within: 'Recent sets',
	reveal: 'Tracklist'
};
