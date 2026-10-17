import { IncludeScope, ScopePermissions, ScopePermissionsTransition, SpacePermission } from '@atproto/oauth-scopes';
import { describe, expect, it } from 'vitest';
import { CALENDAR_EVENT, COLLECTION, PERMISSIONS, SPACE_TYPE } from '#lib/nsid.ts';
import { loadConfig } from '../config';
import { PERMISSION_SET } from './lexicons';
import {
	clientMetadata,
	DECLARED_SCOPES,
	grantsAll,
	grantsPublicRecords,
	grantsSpaces,
	isLoopback,
	SCOPES
} from './oauth';

const config = (publicUrl: string) =>
	loadConfig({
		ROOT_IDENTIFIER: 'did:plc:root',
		ROOT_PASSWORD: 'x',
		PUBLIC_URL: publicUrl,
		HAPPYVIEW_URL: 'http://127.0.0.1:8000'
	});

describe('OAuth scopes', () => {
	const { full, base } = SCOPES;
	const user = 'did:plc:user';
	const root = 'did:plc:root';

	// What the PDS grants for `full`: the permission set expanded, with the planner's `self` authority resolved.
	const granted = [
		...new IncludeScope(PERMISSIONS)
			.toPermissions(PERMISSION_SET)
			.map((permission) =>
				permission instanceof SpacePermission ? permission.withResolvedAuthority(user) : permission
			)
			.map(String),
		...full.split(' ').filter((scope) => !scope.startsWith('include:'))
	];

	it('requests the permission set, keeping explicit only what a set cannot hold', () => {
		expect(full.split(' ').filter((scope) => IncludeScope.fromString(scope)?.nsid === PERMISSIONS)).toHaveLength(1);
		// A permission outside the set's own NSID authority would be dropped silently when the PDS expands it.
		expect(new IncludeScope(PERMISSIONS).toPermissions(PERMISSION_SET)).toHaveLength(PERMISSION_SET.permissions.length);
	});

	it("allows every space write the app makes with a user's session, and no reads of others' records", () => {
		const permissions = new ScopePermissions(granted);
		const audit = { type: SPACE_TYPE.audit, authority: root, skey: 'main' } as const;
		const moderation = { type: SPACE_TYPE.moderation, authority: root, skey: 'main' } as const;
		const booking = { type: SPACE_TYPE.booking, authority: root, skey: '3mxq3whd6gi2d' } as const;
		const planner = { type: SPACE_TYPE.planner, authority: user, skey: 'self' } as const;

		expect(permissions.allowsSpace({ ...audit, action: 'create', collection: COLLECTION.auditEntry })).toBe(true);

		for (const action of ['create', 'update', 'delete'] as const) {
			expect(permissions.allowsSpace({ ...moderation, action, collection: COLLECTION.request })).toBe(true);

			for (const collection of [COLLECTION.bookingInquiry, COLLECTION.bookingMessage, COLLECTION.bookingStatus])
				expect(permissions.allowsSpace({ ...booking, action, collection })).toBe(true);

			for (const collection of [COLLECTION.plannerSave, COLLECTION.plannerShow])
				expect(permissions.allowsSpace({ ...planner, action, collection })).toBe(true);
		}

		for (const manage of ['create', 'update'] as const)
			expect(permissions.allowsSpace({ ...planner, manage })).toBe(true);

		for (const space of [audit, moderation, booking])
			expect(permissions.allowsSpace({ ...space, action: 'read' })).toBe(false);
		expect(permissions.allowsSpace({ ...planner, authority: 'did:plc:someoneelse', action: 'read' })).toBe(false);
		expect(
			permissions.allowsSpace({
				type: SPACE_TYPE.staff,
				authority: root,
				skey: 'main',
				action: 'create',
				collection: COLLECTION.staffRole
			})
		).toBe(false);
	});

	it("lets a DJ run their own press kit spaces, and anyone ask for access to someone else's", () => {
		const permissions = new ScopePermissions(granted);
		const own = { type: SPACE_TYPE.epk, authority: user, skey: '3mabc' } as const;
		const theirs = { ...own, authority: 'did:plc:other' } as const;

		expect(permissions.allowsRepo({ collection: COLLECTION.epk, action: 'create' })).toBe(true);
		expect(permissions.allowsRepo({ collection: COLLECTION.project, action: 'create' })).toBe(false);
		expect(permissions.allowsSpace({ ...own, manage: 'update' })).toBe(true);
		expect(permissions.allowsSpace({ ...own, action: 'create', collection: COLLECTION.epkGrant })).toBe(true);
		expect(permissions.allowsSpace({ ...theirs, action: 'create', collection: COLLECTION.epkRequest })).toBe(true);
		expect(permissions.allowsSpace({ ...theirs, action: 'create', collection: COLLECTION.epkGrant })).toBe(false);
		expect(permissions.allowsSpace({ ...theirs, action: 'read' })).toBe(false);
	});

	it('lets users upload avatars and mix tracklists to their own PDS, and nothing else', () => {
		// The PDS checks uploadBlob against the token's scopes like this.
		const permissions = new ScopePermissionsTransition(full);

		for (const mime of ['image/png', 'image/jpeg', 'image/webp', 'application/json'])
			expect(permissions.allowsBlob({ mime })).toBe(true);
		expect(permissions.allowsBlob({ mime: 'text/html' })).toBe(false);
	});

	it('has a base scope without spaces for fallback, which a full grant covers', () => {
		expect(base.split(' ').some((scope) => scope.startsWith('include:') || scope.startsWith('space'))).toBe(false);
		expect(grantsAll(base, full, user)).toBe(false);
		expect(grantsAll(granted.join(' '), base, user)).toBe(true);
		expect(grantsAll(full, full, user)).toBe(true);
		expect(grantsAll(null, full, user)).toBe(false);
		expect(SCOPES.minimal).toBe('atproto');
	});

	it('lets any account follow, and announce shows as calendar events, with or without spaces', () => {
		for (const scopes of [granted, base.split(' ')]) {
			const permissions = new ScopePermissions(scopes);

			for (const action of ['create', 'delete'] as const)
				expect(permissions.allowsRepo({ collection: COLLECTION.follow, action })).toBe(true);
			expect(permissions.allowsRepo({ collection: COLLECTION.follow, action: 'update' })).toBe(false);

			for (const collection of [COLLECTION.show, CALENDAR_EVENT])
				for (const action of ['create', 'update', 'delete'] as const)
					expect(permissions.allowsRepo({ collection, action })).toBe(true);
			expect(permissions.allowsRepo({ collection: COLLECTION.epk, action: 'create' })).toBe(scopes === granted);
		}
	});

	it('counts a grant from before shows and follows as one with spaces, which lacks only those writes', () => {
		// What the PDS grants from the permission set as it was, or from one it hasn't fetched again yet.
		const publicRecords = (scope: string) => /localnetwork\.(show|follow)|calendar\.event/.test(scope);
		const before = granted.filter((scope) => !publicRecords(scope)).join(' ');

		expect(grantsSpaces(before, user)).toBe(true);
		expect(grantsPublicRecords(before, user)).toBe(false);
		expect(grantsSpaces(granted.join(' '), user)).toBe(true);
		expect(grantsPublicRecords(granted.join(' '), user)).toBe(true);
		expect(grantsSpaces(base, user)).toBe(false);
		expect(grantsPublicRecords(base, user)).toBe(true);
		expect(grantsSpaces(granted.filter((scope) => !scope.startsWith('blob')).join(' '), user)).toBe(false);
	});

	it('declares every scope it may ask for in its client metadata', () => {
		const declared = DECLARED_SCOPES.split(' ');

		for (const scope of Object.values(SCOPES).flatMap((scopes) => scopes.split(' '))) expect(declared).toContain(scope);
		expect(new Set(declared).size).toBe(declared.length);
	});

	it('compares grants the way the PDS writes them, and spots any scope missing', () => {
		expect(grantsAll(granted.join(' '), full, user)).toBe(true);
		expect(grantsAll(granted.join(' '), full, 'did:plc:someoneelse')).toBe(false);

		for (const dropped of granted)
			expect(grantsAll(granted.filter((scope) => scope !== dropped).join(' '), full, user)).toBe(false);
	});

	it('builds loopback metadata for local development and web metadata otherwise', () => {
		expect(isLoopback('http://127.0.0.1:5173')).toBe(true);
		expect(isLoopback('https://localnetwork.computer')).toBe(false);
		const local = clientMetadata(config('http://127.0.0.1:5173'), full);
		expect(local.client_id).toMatch(/^http:\/\/localhost/);
		const web = clientMetadata(config('https://localnetwork.computer'), full);
		expect(web).toMatchObject({
			client_id: 'https://localnetwork.computer/oauth-client-metadata.json',
			redirect_uris: ['https://localnetwork.computer/oauth/callback'],
			dpop_bound_access_tokens: true
		});
	});
});
