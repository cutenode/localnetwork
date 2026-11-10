import { z } from 'zod';
import type { FlagKey } from '#lib/flags/keys.generated.ts';

export const flagDefinition = z
	.object({
		$schema: z.string().optional(),
		key: z
			.string()
			.regex(/^[a-z][a-zA-Z0-9]*$/)
			.max(64)
			.describe('What code asks for: camelCase, the same as the file name.'),
		description: z.string().min(1).max(300).describe('What the flag gates, for the staff console.')
	})
	.strict()
	.describe('A feature flag the website and locallibrary share. Who it is on for is set in the staff console.');

export type FlagDefinition = { key: FlagKey; description: string };

/** A flag's name from its path, which `key` has to match so a search for one finds the other. */
const fileKey = (path: string) => path.slice(path.lastIndexOf('/') + 1, -'.json'.length);

/** The flags in the repo's flags/catalog/, bundled when Vite builds the app. A bad file fails its test and the build. */
export function parseCatalog(files: [path: string, file: unknown][]): FlagDefinition[] {
	return files
		.map(([path, file]) => {
			const flag = flagDefinition.parse(file);

			if (fileKey(path) !== flag.key) throw new Error(`${path} holds the flag ${flag.key}`);

			// SAFETY: scripts/flag-keys.ts generates FlagKey from these same files.
			return { key: flag.key as FlagKey, description: flag.description };
		})
		.toSorted((a, b) => a.key.localeCompare(b.key));
}

export const FLAGS = parseCatalog(
	Object.entries(import.meta.glob('../../../../../../flags/catalog/*.json', { import: 'default', eager: true }))
);
