import { describe, expect, it } from 'vitest';
import { CLAIM_RELATIONSHIPS } from '../src/lib/taxonomy/options.ts';
import { DEMO_CLAIMS, DEMO_DJS, DEMO_MANAGERS, DEMO_PROJECTS, managersOf } from './seed-data.ts';

describe('the demo claims', () => {
	it('give every demo project a manager', () => {
		const unmanaged = DEMO_PROJECTS.filter((_, index) => !managersOf(index).length).map(({ name }) => name);

		expect(unmanaged).toEqual([]);
	});

	it("name each solo project's DJ with a self claim", () => {
		const solo = DEMO_PROJECTS.flatMap((project, index) => (project.kind === 'solo' ? [index] : []));

		const selfClaims = solo.map(
			(index) => DEMO_CLAIMS.filter((claim) => claim.project === index && claim.relationship === 'self').length
		);

		expect(solo).toHaveLength(7);
		expect(selfClaims).toEqual(solo.map(() => 1));
	});

	it('come from accounts the seed makes, once per project, on projects that exist', () => {
		const accounts = new Set([
			'neon.test',
			...DEMO_DJS.map(({ handle }) => handle),
			...DEMO_MANAGERS.map(({ handle }) => handle)
		]);

		const relationships = new Set<string>(CLAIM_RELATIONSHIPS.map(({ value }) => value));
		const pairs = DEMO_CLAIMS.map(({ handle, project }) => `${handle} ${project}`);

		expect(DEMO_CLAIMS.filter(({ handle }) => !accounts.has(handle))).toEqual([]);
		expect(DEMO_CLAIMS.filter(({ project }) => !DEMO_PROJECTS[project])).toEqual([]);
		expect(DEMO_CLAIMS.filter(({ relationship }) => !relationships.has(relationship))).toEqual([]);
		expect(new Set(pairs).size).toBe(pairs.length);
	});

	it('leave nobody the seed makes to manage without managing anything', () => {
		const idle = DEMO_MANAGERS.filter(({ handle }) => !DEMO_CLAIMS.some((claim) => claim.handle === handle));

		expect(idle).toEqual([]);
	});

	it("list a project's managers in the order they claimed it, so the first is its creator", () => {
		expect(managersOf(0)).toEqual(['neon.test']);
		expect(managersOf(1)).toEqual(['neon.test']);
		expect(managersOf(8)).toEqual(['harbour.test']);
	});
});
