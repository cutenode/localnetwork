import { redirect } from '@sveltejs/kit';
import { describe, expect, it } from 'vitest';
import type { StaffRole, Viewer } from '#lib/types.ts';
import { RecordValidationError } from './atproto/lexicons';
import { UserError } from './core';
import {
	act,
	all,
	cacheBriefly,
	clientIp,
	limit,
	optionalCurrency,
	requireAdmin,
	requireStaff,
	requireViewer,
	safeNext,
	stringValues,
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
		expect(failed).toMatchObject({ status: 409, data: { error: 'Nope', values } });
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
		expect(safeNext('//evil.test')).toBe('/me');
		expect(safeNext('/\\evil.test')).toBe('/me');
		expect(safeNext('https://evil.test', '/')).toBe('/');
		expect(safeNext(null)).toBe('/me');
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
	it('lets anyone cache a page for a minute, but only the browser when someone is signed in', () => {
		const headers = (viewer: Viewer | null) => {
			let set: Record<string, string> = {};
			cacheBriefly({ locals: { viewer }, setHeaders: (value) => (set = value) });

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

		expect(headers(null)).toEqual({ 'cache-control': 'public, max-age=60' });
		expect(headers(signedIn)).toEqual({ 'cache-control': 'private, max-age=60' });
	});
});
