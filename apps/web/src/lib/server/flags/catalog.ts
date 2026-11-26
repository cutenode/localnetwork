import { z } from 'zod';
import type { FlagKey } from '#lib/flags/keys.generated.ts';
import { SHOWS_ON_KINDS, type ShowsOn } from '#lib/flags/shows-on.ts';

const flagFields = {
	$schema: z.string().optional(),
	key: z
		.string()
		.regex(/^[a-z][a-zA-Z0-9]*$/)
		.max(64)
		.describe('What code asks for: camelCase, the same as the file name.'),
	description: z.string().min(1).max(300).describe('What the flag gates, for the staff console.'),
	showsOn: z
		.array(z.enum(SHOWS_ON_KINDS))
		.min(1)
		.optional()
		.describe(
			'The kinds of page the flag changes, or global for every page. act is a /projects page until acts move. The staff console lists them as Shows on.'
		)
};

/** Release flags are meant to go, so they carry a date; ops flags stay and can't. */
export const flagDefinition = z
	.discriminatedUnion('type', [
		z
			.object({
				...flagFields,
				type: z.literal('release').describe('release rolls a feature out, and goes once it has.'),
				removeBy: z.iso
					.date()
					.describe('When the flag and the code it gates should be gone, as YYYY-MM-DD. Tests fail after it.')
			})
			.strict(),
		z
			.object({
				...flagFields,
				type: z
					.literal('ops')
					.describe(
						'ops changes how the site runs, and stays. A kill switch is ops and named for its paused state, like pauseRecordingStream.'
					)
			})
			.strict()
	])
	.describe('A feature flag the website and locallibrary share. Who it is on for is set in the staff console.');

export type FlagDefinition = { key: FlagKey; description: string; showsOn?: ShowsOn[] } & (
	| { type: 'release'; removeBy: string }
	| { type: 'ops'; removeBy?: undefined }
);

/** A flag's name from its path, which `key` has to match so a search for one finds the other. */
const fileKey = (path: string) => path.slice(path.lastIndexOf('/') + 1, -'.json'.length);

/** The flags in the repo's flags/catalog/, bundled when Vite builds the app. A bad file fails its test and the build. */
export function parseCatalog(files: [path: string, file: unknown][]): FlagDefinition[] {
	return files
		.map(([path, file]) => {
			const { $schema: _schema, ...flag } = flagDefinition.parse(file);

			if (fileKey(path) !== flag.key) throw new Error(`${path} holds the flag ${flag.key}`);

			// SAFETY: scripts/flag-keys.ts generates FlagKey from these same files.
			return { ...flag, key: flag.key as FlagKey };
		})
		.toSorted((a, b) => a.key.localeCompare(b.key));
}

/**
 * Why each flag past its removeBy on `today` (YYYY-MM-DD, UTC) should go. Only tests call this: a date passing must
 * not break a build or a deploy.
 * @internal
 */
export function overdue(flags: FlagDefinition[], today: string): string[] {
	return flags.flatMap(({ key, removeBy }) =>
		removeBy !== undefined && removeBy < today
			? [
					`${key} was due for removal on ${removeBy}: delete flags/catalog/${key}.json and the code it gates, or move removeBy`
				]
			: []
	);
}

export const FLAGS = parseCatalog(
	Object.entries(import.meta.glob('../../../../../../flags/catalog/*.json', { import: 'default', eager: true }))
);
