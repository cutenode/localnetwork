import { describe, expect, it } from 'vitest';
import { expiredBackups, isBackupDue, type StoredBackup } from './retention';

const at = (iso: string): StoredBackup => ({ key: iso, takenAt: new Date(iso) });

/** One backup a day at 00:10 UTC, for `days` days up to and including `last`. */
const daily = (last: string, days: number) =>
	Array.from({ length: days }, (_, i) => {
		const date = new Date(`${last}T00:10:00Z`);
		date.setUTCDate(date.getUTCDate() - i);

		return at(date.toISOString());
	});

const kept = (backups: StoredBackup[], policy: { daily: number; weekly: number }) => {
	const expired = new Set(expiredBackups(backups, policy).map((backup) => backup.key));

	return backups.flatMap((backup) => (expired.has(backup.key) ? [] : [backup.key.slice(0, 10)])).sort();
};

describe('expiredBackups', () => {
	it('keeps everything when there are fewer backups than the policy allows', () => {
		expect(expiredBackups([], { daily: 7, weekly: 8 })).toEqual([]);
		expect(expiredBackups(daily('2026-10-30', 3), { daily: 7, weekly: 0 })).toEqual([]);
	});

	it('keeps the newest backup of each of the last N days', () => {
		const backups = [...daily('2026-10-30', 5), at('2026-10-30T12:00:00.000Z'), at('2026-10-29T18:00:00.000Z')];
		const expired = expiredBackups(backups, { daily: 3, weekly: 0 }).map((backup) => backup.key);

		expect(expired.sort()).toEqual([
			'2026-10-26T00:10:00.000Z',
			'2026-10-27T00:10:00.000Z',
			// Earlier the same day as a newer one.
			'2026-10-29T00:10:00.000Z',
			'2026-10-30T00:10:00.000Z'
		]);
	});

	it('also keeps the newest backup of each of the last M weeks, which start on Mondays', () => {
		// 2026-10-30 is a Friday, so the weeks start on 10-26, 10-19, 10-12 and 10-05. 10-30 is both
		// a daily backup and its week's newest.
		expect(kept(daily('2026-10-30', 30), { daily: 2, weekly: 4 })).toEqual([
			'2026-10-11',
			'2026-10-18',
			'2026-10-25',
			'2026-10-29',
			'2026-10-30'
		]);
	});

	it('counts only days and weeks that have a backup, so a gap keeps older ones', () => {
		const backups = [at('2026-10-30T00:10:00.000Z'), at('2026-10-10T00:10:00.000Z'), at('2026-09-01T00:10:00.000Z')];

		expect(kept(backups, { daily: 2, weekly: 0 })).toEqual(['2026-10-10', '2026-10-30']);
	});

	it('puts a Sunday and the Monday after it in different weeks', () => {
		const sunday = at('2026-10-25T23:59:00.000Z');
		const monday = at('2026-10-26T00:01:00.000Z');

		expect(expiredBackups([sunday, monday], { daily: 1, weekly: 2 })).toEqual([]);
		expect(
			expiredBackups([at('2026-10-26T00:01:00.000Z'), at('2026-11-01T23:00:00.000Z')], { daily: 1, weekly: 1 })
		).toEqual([at('2026-10-26T00:01:00.000Z')]);
	});

	it('always keeps the newest backup, whatever order the bucket listed them in', () => {
		const backups = daily('2026-10-30', 10).toReversed();

		expect(kept(backups, { daily: 1, weekly: 0 })).toEqual(['2026-10-30']);
	});
});

describe('isBackupDue', () => {
	const now = new Date('2026-10-30T00:30:00Z');

	it('is due when there is no backup yet', () => {
		expect(isBackupDue(undefined, now)).toBe(true);
	});

	it('is due once per UTC day', () => {
		expect(isBackupDue(new Date('2026-10-29T23:59:00Z'), now)).toBe(true);
		expect(isBackupDue(new Date('2026-10-30T00:00:00Z'), now)).toBe(false);
	});
});
