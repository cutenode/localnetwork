import { redirect } from '@sveltejs/kit';
import { describe, expect, it } from 'vitest';
import type { StaffRole, Viewer } from '#lib/types.ts';
import { RecordValidationError } from './atproto/lexicons';
import { ReauthError, UserError } from './core';
import {
	act,
	all,
	cacheBriefly,
	clientIp,
	guardArea,
	limit,
	optionalCurrency,
	requireAdmin,
	requireStaff,
	requireViewer,
	safeNext,
	stringValues,
	withQuery,
	wholeNumber
} from './http';

function form(entries: [string, string | File][]): FormData {
	const data = new FormData();

	for (const [key, value] of entries) data.append(key, value);

	return data;
}

describe('form helpers', () => {
	it('echoes text values, keeping the last of a repeated field and skipping files', () => {
		expect(
			stringValues(
				form([
					['title', 'Echo'],
					['slot', 'a'],
					['slot', 'b'],
					['file', new File(['x'], 'x.txt')]
				])
			)
		).toEqual({ title: 'Echo', slot: 'b' });
	});

	it('parses currency codes and whole numbers, or says what is wrong', () => {
		expect(optionalCurrency(form([['currency', ' eur ']]), 'currency')).toBe('EUR');
		expect(optionalCurrency(form([['currency', '']]), 'currency')).toBeUndefined();
		expect(() => optionalCurrency(form([['currency', 'EURO']]), 'currency')).toThrow(/three-letter currency/);
		expect(wholeNumber('', 'year', 1950, 2100)).toBeUndefined();
		expect(wholeNumber('2024', 'year', 1950, 2100)).toBe(2024);
		expect(() => wholeNumber('20.5', 'year', 1950, 2100)).toThrow(/year must be a whole number from 1950 to 2100/);
	});

	it('counts graphemes, not code units, against a limit', () => {
		expect(limit('Name', '👩‍🎤'.repeat(3), 3)).toBe('👩‍🎤'.repeat(3));
		expect(limit('Name', undefined, 3)).toBeUndefined();
		expect(() => limit('Name', 'abcd', 3)).toThrow('Name can be at most 3 characters');
	});
});

describe('act', () => {
	it('turns user and record errors into failures that echo the values', async () => {
		const values = { title: 'Echo' };
		const failed = await act(() => Promise.reject(new UserError('Nope', 409)), values);
		expect(failed).toMatchObject({ status: 409, data: { error: 'Nope', values, reauth: false } });
		const lapsed = await act(() => Promise.reject(new ReauthError('Sign in again to follow DJs')));
		expect(lapsed).toMatchObject({ status: 401, data: { error: 'Sign in again to follow DJs', reauth: true } });
		const invalid = await act(() => Promise.reject(new RecordValidationError('x', 'too long')), values);
		expect(invalid).toMatchObject({ status: 400, data: { error: expect.stringMatching(/record format/), values } });
		expect(await act(() => Promise.resolve({ ok: true }))).toEqual({ ok: true });
	});

	it('lets redirects and unexpected errors through', async () => {
		await expect(act(() => Promise.reject(new Error('boom')))).rejects.toThrow('boom');
		await expect(
			act(async () => {
				redirect(303, '/elsewhere');
			})
		).rejects.toMatchObject({ status: 303, location: '/elsewhere' });
	});
});

describe('safeNext', () => {
	it('only allows same-site paths', () => {
		expect(safeNext('/projects?q=a')).toBe('/projects?q=a');
		expect(safeNext('//evil.test')).toBe('/dashboard');
		expect(safeNext('/\\evil.test')).toBe('/dashboard');
		expect(safeNext('https://evil.test', '/')).toBe('/');
		expect(safeNext(null)).toBe('/dashboard');
	});

	it('refuses paths a browser would read as another site', () => {
		// Browsers drop tabs and newlines from a URL and read a backslash as a slash, so each of
		// these becomes `//evil.test`.
		for (const next of ['/\t/evil.test', '/\n/evil.test', '/\r\n/evil.test', '/\\\\evil.test', '/x\\..\\\\evil.test']) {
			expect(safeNext(next)).toBe('/dashboard');
		}

		expect(safeNext('/ /evil.test')).toBe('/dashboard');
		expect(safeNext('/%09/evil.test')).toBe('/%09/evil.test');
	});

	it('hands back the path as a browser would resolve it, so building on it stays on the site', () => {
		// Each resolves to the path `//evil.test`, which a redirect built from the parsed URL would send off the site.
		for (const next of ['/.//evil.test', '/x/..//evil.test', '/%2e//evil.test']) {
			expect(safeNext(next)).toBe('/dashboard');
			expect(withQuery(next, { email: 'declined' })).toBe('/dashboard');
		}

		expect(safeNext('/x/../inbox?a=1#top')).toBe('/inbox?a=1#top');
		expect(safeNext('/%2F%2Fevil.test')).toBe('/%2F%2Fevil.test');
		expect(withQuery('/dashboard/inbox/settings?tab=1', { email: 'declined' })).toBe(
			'/dashboard/inbox/settings?tab=1&email=declined'
		);
	});
});

describe('guards', () => {
	const viewer = (...roles: StaffRole[]): Viewer => ({
		did: 'did:plc:a',
		handle: null,
		roles,
		isAdmin: roles.includes('admin'),
		isStaff: roles.length > 0,
		spacesCapable: true
	});

	const thrown = (run: () => void) => {
		try {
			run();
		} catch (cause) {
			return cause;
		}
	};

	const url = new URL('https://x.test/admin?tab=1');

	it('sends signed-out people to sign in and refuses the wrong role', () => {
		expect(thrown(() => requireViewer({ viewer: null }, url))).toMatchObject({
			status: 303,
			location: '/login?next=%2Fadmin%3Ftab%3D1'
		});
		expect(thrown(() => requireStaff({ viewer: viewer() }, url))).toMatchObject({ status: 403 });
		expect(thrown(() => requireAdmin({ viewer: viewer('moderator') }, url))).toMatchObject({ status: 403 });
		expect(requireStaff({ viewer: viewer('moderator') }, url).did).toBe('did:plc:a');
		expect(requireAdmin({ viewer: viewer('admin') }, url).isAdmin).toBe(true);
	});

	it('guards the dashboard and the staff console by the route matched, and nothing else', () => {
		const guard = (viewer: Viewer | null, path: string, route: string | null) =>
			thrown(() => guardArea({ viewer }, new URL(path, 'https://x.test'), route));

		expect(guard(null, '/dashboard/bookings?x=1', '/dashboard/bookings')).toMatchObject({
			status: 303,
			location: '/login?next=%2Fdashboard%2Fbookings%3Fx%3D1'
		});
		expect(guard(null, '/dashboard', '/dashboard')).toMatchObject({ status: 303 });
		expect(guard(viewer(), '/dashboard/planner', '/dashboard/planner')).toBeUndefined();
		expect(guard(null, '/admin/requests', '/admin/requests')).toMatchObject({
			status: 303,
			location: '/login?next=%2Fadmin%2Frequests'
		});
		expect(guard(viewer(), '/admin', '/admin')).toMatchObject({ status: 403 });
		expect(guard(viewer(), '/admin/audit/export', '/admin/audit/export')).toMatchObject({ status: 403 });
		expect(guard(viewer('moderator'), '/admin/requests', '/admin/requests')).toBeUndefined();
		// SvelteKit matches routes on the decoded path, so an encoded one still reaches the area, and the
		// sign-in link keeps the path as it came.
		expect(guard(viewer(), '/%61dmin/requests', '/admin/requests')).toMatchObject({ status: 403 });
		expect(guard(null, '/%64ashboard/bookings', '/dashboard/bookings')).toMatchObject({
			status: 303,
			location: '/login?next=%2F%2564ashboard%2Fbookings'
		});
		// Only whole route segments count, and public pages and unmatched paths stay open.
		expect(guard(null, '/dashboards', null)).toBeUndefined();
		expect(guard(viewer(), '/projects/neon-tide', '/projects/[slug]')).toBeUndefined();
		expect(guard(viewer(), '/administer', null)).toBeUndefined();
	});

	it('reads repeated fields and the client address', () => {
		expect(
			all(
				form([
					['g', ' a '],
					['g', ''],
					['g', 'b'],
					['g', 'c']
				]),
				'g',
				2
			)
		).toEqual(['a', 'b']);
		expect(clientIp({ getClientAddress: () => '10.0.0.1' })).toBe('10.0.0.1');
		expect(
			clientIp({
				getClientAddress: () => {
					throw new Error('no address');
				}
			})
		).toBe('unknown');
	});
});

describe('cacheBriefly', () => {
	it('lets anyone cache a page for a minute, but only the browser when the request has a session', () => {
		const headers = (viewer: Viewer | null, sessionToken?: string) => {
			let set: Record<string, string> = {};
			cacheBriefly({ locals: { viewer, sessionToken }, setHeaders: (value) => (set = value) });

			return set;
		};

		const signedIn: Viewer = {
			did: 'did:plc:pat',
			handle: 'pat.test',
			roles: [],
			isAdmin: false,
			isStaff: false,
			spacesCapable: true
		};

		expect(headers(null)).toEqual({ 'cache-control': 'public, max-age=60', vary: 'Cookie' });
		expect(headers(signedIn, 'token')).toEqual({ 'cache-control': 'private, max-age=60', vary: 'Cookie' });
		// A session that ended: no viewer, but the cookie is there (to be cleared, or to ask to sign in again).
		expect(headers(null, 'ended')).toEqual({ 'cache-control': 'private, max-age=60', vary: 'Cookie' });
	});
});
