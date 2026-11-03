import { IncludeScope } from '@atproto/oauth-scopes';
import { describe, expect, it, vi } from 'vitest';
import hostList from '#lib/hosts.json' with { type: 'json' };
import { PERMISSIONS } from '#lib/nsid.ts';
import { CAN } from '#lib/sign-in.ts';
import type { ProjectView } from '#lib/types.ts';
import { permissionSetFor } from './atproto/lexicons';
import { DECLARED_SCOPES } from './atproto/oauth';
import { silentLogger } from './logger';
import { accountHosts, signInPurpose } from './sign-in';

// SAFETY: signInPurpose reads only a project's status and name.
const project = (slug: string, name: string, status: ProjectView['status'] = 'active') =>
	({ slug, status, profile: { name } }) as ProjectView;

const projects = new Map([
	['neon-tide', project('neon-tide', 'Neon Tide')],
	['hidden-crew', project('hidden-crew', 'Hidden Crew', 'hidden')]
]);

const deps = { bySlug: async (slug: string) => projects.get(slug) ?? null, log: silentLogger };

describe('signInPurpose', () => {
	it.each([
		['/projects/neon-tide', 'book Neon Tide'],
		['/projects/neon-tide/press-kits', 'book Neon Tide'],
		['/projects/neon-tide#follow', 'follow Neon Tide'],
		['/dashboard/bookings/new?project=neon-tide', 'book Neon Tide'],
		['/shows?view=following', 'see shows from DJs you follow'],
		['/projects/hidden-crew', null],
		['/projects/nobody', null],
		['/dashboard/bookings/new', null],
		['/shows', null],
		['/dashboard', null]
	])('says %s is to %s', async (next, purpose) => {
		expect(await signInPurpose(next, deps)).toBe(purpose);
	});

	it("says nothing when the directory can't name the project", async () => {
		const bySlug = vi.fn<(slug: string) => Promise<ProjectView | null>>(async () => {
			throw new Error('offline');
		});

		expect(await signInPurpose('/projects/neon-tide', { bySlug, log: silentLogger })).toBeNull();
	});
});

describe('accountHosts', () => {
	it('offers hosts.json, and a dev network its own PDS first', () => {
		expect(
			accountHosts({ dev: false, root: { identifier: 'r', password: 'p', service: 'http://127.0.0.1:2583' } })
		).toEqual(hostList.hosts);
		expect(
			accountHosts({ dev: true, root: { identifier: 'r', password: 'p', service: 'https://pds.example' } })
		).toEqual(hostList.hosts);
		expect(accountHosts({ dev: true, root: { identifier: 'r', password: 'p' } })).toEqual(hostList.hosts);

		const [local, ...rest] = accountHosts({
			dev: true,
			root: { identifier: 'r', password: 'p', service: 'http://127.0.0.1:2583' }
		});

		expect(local).toMatchObject({ name: 'This dev network', service: 'http://127.0.0.1:2583' });
		expect(rest).toEqual(hostList.hosts);
	});

	// hosts.schema.json checks this in an editor; this keeps a bad edit from reaching the sign-up list.
	it('lists each host once, at an https origin', () => {
		const services = hostList.hosts.map((host) => host.service);

		expect(new Set(services).size).toBe(services.length);
		expect(
			services.filter((service) => new URL(service).origin !== service || !service.startsWith('https://'))
		).toEqual([]);
	});
});

describe('CAN', () => {
	it('explains every collection, space and upload the app may ask for', () => {
		const scopes = [
			...DECLARED_SCOPES.split(' '),
			...new IncludeScope(PERMISSIONS).toScopes(permissionSetFor('did:plc:root'))
		];

		const resources = scopes.flatMap((scope) =>
			scope === 'atproto' || scope.startsWith('include:')
				? []
				: [scope.match(/^(?:repo|space):([^?]+)/)?.[1] ?? scope.split('?')[0]]
		);

		const covered = new Set(CAN.flatMap((line) => line.covers));

		expect(resources.length).toBeGreaterThan(5);
		expect(resources.filter((resource) => !covered.has(resource))).toEqual([]);
	});
});
