import * as fc from 'fast-check';
import { describe, expect, it } from 'vitest';
import peopleDefs from '../../../../lexicons/computer/localnetwork/people/defs.json' with { type: 'json' };
import {
	CAPABILITIES,
	capabilitiesFor,
	capabilitiesIn,
	CREATOR_ROLE,
	isRole,
	isWriter,
	ROLES,
	SPACE_KINDS,
	type SpaceKind
} from './capabilities';

const roles = SPACE_KINDS.flatMap((kind) => Object.keys(ROLES[kind]).map((role) => ({ kind, role })));

describe('the people defs lexicon', () => {
	it('lists every capability and role the site knows, so other readers know them', () => {
		expect(peopleDefs.defs.capability.knownValues).toEqual(Object.keys(CAPABILITIES));
		expect(peopleDefs.defs.role.knownValues).toEqual([...new Set(roles.map(({ role }) => role))]);
	});
});

describe('roles', () => {
	it.each(roles)('$kind $role starts with only capabilities its kind offers', ({ kind, role }) => {
		if (!isRole(kind, role)) throw new Error(`${role} isn't a ${kind} role`);
		const capabilities = capabilitiesFor(kind, role);

		expect(capabilities.length).toBeGreaterThan(0);
		expect(capabilities.filter((capability) => !CAPABILITIES[capability].kinds.some((k) => k === kind))).toEqual([]);
	});

	it('gives the person who starts a space every capability of its kind, people.manage included', () => {
		for (const kind of SPACE_KINDS) {
			const all = Object.entries(CAPABILITIES).filter(([, spec]) => spec.kinds.some((k) => k === kind));

			expect(capabilitiesFor(kind, CREATOR_ROLE[kind])).toEqual(all.map(([capability]) => capability));
		}
	});

	it("keeps a performer's managers and a party's cohosts from managing its people", () => {
		expect(capabilitiesFor('performer', 'manager')).not.toContain('people.manage');
		expect(capabilitiesFor('party', 'cohost')).not.toContain('people.manage');
		expect(capabilitiesFor('party', 'cohost')).toHaveLength(capabilitiesFor('party', 'host').length - 1);
	});

	it("knows only each kind's own roles", () => {
		expect(isRole('party', 'host')).toBe(true);
		expect(isRole('performer', 'host')).toBe(false);
		expect(isRole('venue', 'toString')).toBe(false);
	});

	it('makes a writer of anyone with a capability that writes, and only them', () => {
		expect(isWriter(capabilitiesFor('performer', 'manager'))).toBe(true);
		expect(isWriter(capabilitiesFor('party', 'door'))).toBe(false);
		expect(isWriter(capabilitiesFor('venue', 'booker'))).toBe(false);
		expect(isWriter([])).toBe(false);
	});
});

describe('capabilitiesIn', () => {
	const kind = fc.constantFrom<SpaceKind>(...SPACE_KINDS);
	const values = fc.array(fc.oneof(fc.constantFrom(...Object.keys(CAPABILITIES)), fc.string()));

	it("reads the same set the same way, whatever its order or repeats, keeping only the kind's capabilities", () => {
		fc.assert(
			fc.property(kind, values, (spaceKind, list) => {
				const read = capabilitiesIn(spaceKind, list);

				expect(capabilitiesIn(spaceKind, [...list.toReversed(), ...list])).toEqual(read);
				expect(capabilitiesIn(spaceKind, read)).toEqual(read);
				expect(read.every((capability) => list.includes(capability))).toBe(true);
				expect(read.every((capability) => CAPABILITIES[capability].kinds.some((k) => k === spaceKind))).toBe(true);
			})
		);
	});
});
