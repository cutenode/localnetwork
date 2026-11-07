import fc from 'fast-check';
import { describe, expect, it } from 'vitest';
import vectors from '../../../../../../flags/vectors.json' with { type: 'json' };
import { bucket, isOn } from './evaluate';

const did = fc.stringMatching(/^did:plc:[a-z2-7]{24}$/);

const key = fc.stringMatching(/^[a-z][a-zA-Z0-9]{0,20}$/);

const percentage = fc.integer({ min: 0, max: 100 });

describe('the rule locallibrary shares', () => {
	it.each(vectors.cases)('$key for $did at $percentage%', (vector) => {
		const { key, did, everyone, targeted, percentage, bucket: expected, on } = vector;

		expect(did === null ? null : bucket(key, did)).toBe(expected);
		expect(isOn({ everyone, targeted, percentage }, key, did)).toBe(on);
	});
});

describe('rollouts', () => {
	it('keeps every account in as the percentage grows', () => {
		fc.assert(
			fc.property(key, did, percentage, percentage, (flag, account, a, b) => {
				const [low, high] = a < b ? [a, b] : [b, a];
				const gates = { everyone: false, targeted: false };

				const stays =
					!isOn({ ...gates, percentage: low }, flag, account) || isOn({ ...gates, percentage: high }, flag, account);

				expect(stays).toBe(true);
			})
		);
	});

	it('takes in every account at 100 and none at 0', () => {
		fc.assert(
			fc.property(key, did, (flag, account) => {
				const gates = { everyone: false, targeted: false };

				expect(isOn({ ...gates, percentage: 100 }, flag, account)).toBe(true);
				expect(isOn({ ...gates, percentage: 0 }, flag, account)).toBe(false);
			})
		);
	});

	it('never counts a signed-out visitor', () => {
		fc.assert(
			fc.property(key, percentage, (flag, share) => {
				expect(isOn({ everyone: false, targeted: false, percentage: share }, flag, null)).toBe(false);
			})
		);
	});

	it('moves an account to a different bucket for a different flag', () => {
		const account = 'did:plc:z72i7hdynmk6r22z27h6tvur';

		expect(new Set(['a', 'b', 'c', 'd', 'e', 'f'].map((flag) => bucket(flag, account))).size).toBeGreaterThan(1);
	});
});
