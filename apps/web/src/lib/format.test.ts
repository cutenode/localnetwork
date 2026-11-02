import { describe, expect, it, vi } from 'vitest';
import {
	accountLabel,
	feeLabel,
	feeLevels,
	feeSummary,
	formatDate,
	formatShowDate,
	hueFor,
	initials,
	kindLabel,
	placeLabel,
	pluralize,
	requestTitle,
	shortDid,
	timeAgo
} from './format';
import { request } from './testing/requests';
import type { ProjectProfile } from './types';

const profile = (fees?: ProjectProfile['fees']): ProjectProfile => ({
	name: 'X',
	kind: 'duo',
	genres: [],
	location: { city: 'Leeds', region: 'England', country: 'GB' },
	fees
});

describe('format helpers', () => {
	it('labels accounts and DIDs', () => {
		expect(accountLabel({ did: 'did:plc:abc', handle: 'a.test' })).toBe('@a.test');
		expect(accountLabel({ did: 'did:plc:abcdefghijklmnopqrstuvwxyz', handle: null })).toBe('did:plc:abcdef…uvwxyz');
		expect(accountLabel(null)).toBe('unknown');
		expect(shortDid('did:plc:short')).toBe('did:plc:short');
	});

	it('formats relative and absolute times', () => {
		const now = Date.parse('2026-10-04T12:00:00Z');
		expect(timeAgo('2026-10-04T11:59:30Z', now)).toBe('just now');
		expect(timeAgo('2026-10-04T10:00:00Z', now)).toBe('2 hours ago');
		expect(timeAgo('2026-10-11T12:00:00Z', now)).toBe('next week');
		expect(timeAgo(null)).toBe('');
		expect(formatDate('2026-10-04T12:00:00Z')).toBe('Oct 4, 2026');
		expect(formatDate('nonsense')).toBe('');
	});

	it('reads fee levels, and a record from before fee levels as one', () => {
		expect(feeLevels(profile())).toEqual([]);
		expect(feeLevels(profile([{ tier: 'emerging' }, { tier: 'headliner' }]))).toHaveLength(2);
		expect(feeLevels({ ...profile(), fee: { tier: 'established' } })).toEqual([{ tier: 'established' }]);
		expect(feeLevels({ ...profile([{ tier: 'emerging' }]), fee: { tier: 'established' } })).toEqual([
			{ tier: 'emerging' }
		]);
	});

	it('formats fees, kinds, places, and counts', () => {
		expect(feeLabel({ tier: 'emerging' })).toBe('Emerging');
		expect(feeLabel({ tier: 'established', currency: 'GBP', minAmount: 200, maxAmount: 500 })).toBe(
			'Established · £200–£500'
		);
		expect(feeLabel({ tier: 'community', minAmount: 50 })).toBe('Community · from 50');
		expect(feeLabel({ tier: 'community', maxAmount: 80 })).toBe('Community · up to 80');
		expect(feeSummary(profile())).toBe('');
		expect(feeSummary(profile([{ tier: 'emerging' }]))).toBe('Emerging');
		expect(feeSummary(profile([{ tier: 'emerging' }, { tier: 'headliner' }, { tier: 'community' }]))).toBe(
			'Emerging +2'
		);
		expect(kindLabel('duo')).toBe('Duo / B2B pair');
		expect(placeLabel(profile())).toBe('Leeds, England, GB');
		expect(placeLabel({ ...profile(), location: { city: 'Berlin', region: 'Berlin', country: 'DE' } })).toBe(
			'Berlin, DE'
		);
		expect(pluralize(1, 'project')).toBe('1 project');
		expect(pluralize(1200, 'city', 'cities')).toBe('1,200 cities');
	});

	it('derives initials and stable hues', () => {
		expect(initials('Neon Tide')).toBe('NT');
		expect(initials('saffron')).toBe('SA');
		expect(initials('!!!')).toBe('?');
		expect(hueFor('abc')).toBe(hueFor('abc'));
		expect(hueFor('abc')).toBeGreaterThanOrEqual(0);
	});

	it('shows show dates as the wall-clock time that was entered', () => {
		vi.stubEnv('TZ', 'Asia/Tokyo');

		try {
			expect(formatDate('2026-12-12T20:00:00.000Z')).toBe('Dec 13, 2026');
			expect(formatShowDate('2026-12-12T20:00:00.000Z')).toBe('Dec 12, 2026');
			expect(formatShowDate('2026-12-12T23:30:00.000Z', true)).toMatch(/^Dec 12, 2026, 11:30\sPM$/);
			expect(formatShowDate(null)).toBe('');
		} finally {
			vi.unstubAllEnvs();
		}
	});
});

describe('requestTitle', () => {
	it('names the kind and the project, the proposed name for a new one, or the requester for a listing', () => {
		expect(requestTitle(request('update'))).toBe('Changes: Neon Tide');
		expect(requestTitle(request('claim'))).toBe('Claim: Neon Tide');
		expect(requestTitle(request('create', { project: null }))).toBe('New project: Neon Tide');
		expect(
			requestTitle(request('create', { project: null, record: { kind: 'create', createdAt: '2026-10-01T00:00:00Z' } }))
		).toBe('New project: Project');
		expect(requestTitle(request('list', { project: null }))).toBe('Listing: @neon.test');
	});
});
