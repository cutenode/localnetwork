// Plain Node runs this directory as the db:* scripts (Node strips the types itself), so its files
// import each other with their .ts extension and use only syntax Node can strip.

/** How many backups to keep: the newest of each of the last `daily` UTC days, and of the last `weekly` weeks. */
export type RetentionPolicy = { daily: number; weekly: number };

export type StoredBackup = { key: string; takenAt: Date };

const DAY_MS = 86_400_000;

const utcDay = (date: Date) => Math.floor(date.getTime() / DAY_MS);

// 1970-01-01 was a Thursday, so shifting by three days starts each week on a Monday, as ISO weeks do.
const utcWeek = (date: Date) => Math.floor((utcDay(date) + 3) / 7);

/**
 * The backups the policy no longer keeps. Days and weeks count only those that have a backup, so a
 * gap (the app was down) doesn't use up the allowance. The newest backup is always kept.
 */
export function expiredBackups(backups: StoredBackup[], policy: RetentionPolicy): StoredBackup[] {
	const newestFirst = [...backups].sort((a, b) => b.takenAt.getTime() - a.takenAt.getTime());
	const days = new Set<number>();
	const weeks = new Set<number>();

	return newestFirst.filter((backup) => {
		const day = utcDay(backup.takenAt);
		const week = utcWeek(backup.takenAt);
		const isDaily = !days.has(day) && days.size < policy.daily;
		const isWeekly = !weeks.has(week) && weeks.size < policy.weekly;

		if (isDaily) days.add(day);

		if (isWeekly) weeks.add(week);

		return !isDaily && !isWeekly;
	});
}

/** Whether a backup is due: none was taken yet on `now`'s UTC day. */
export const isBackupDue = (latest: Date | undefined, now: Date) => !latest || utcDay(latest) < utcDay(now);
