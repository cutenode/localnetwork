import { FLAGS } from '#lib/server/flags/catalog.ts';
import type { FlagAnswer } from '#lib/server/services/flags.ts';
import type { Flags } from '#lib/types.ts';

/**
 * Every flag in flags/catalog/, off: what anyone gets before a flag is turned on for them. Tests spread it under the
 * flags they turn on, so adding a flag to the catalog doesn't change what they expect.
 */
// SAFETY: FLAGS has one entry for every FlagKey.
export const flagsOff = Object.fromEntries(FLAGS.map(({ key }) => [key, false])) as Flags;

/** What `answers()` gives with `key`'s gates as `gates` and every other flag in the catalog untouched. */
export const answersWith = (key: string, gates: Omit<FlagAnswer, 'key'>): FlagAnswer[] =>
	FLAGS.map((flag) =>
		flag.key === key
			? { key: flag.key, ...gates }
			: { key: flag.key, enabled: true, everyone: false, percentage: 0, targeted: false }
	);
