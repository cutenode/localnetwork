import { readdirSync } from 'node:fs';
import type { Kind } from '../../src/lib/server/notifications/kind.ts';

const folder = new URL('../../src/lib/server/notifications/kinds/', import.meta.url);

/**
 * Every kind of notification in kinds/, imported from disk and keyed by path as `import.meta.glob` keys them. This is
 * what `#notification-kinds` resolves to outside Vite (the dev network and the other tsx scripts); under Vite it's
 * src/lib/server/notifications/kind-files.ts.
 */
export const KIND_FILES: Record<string, Kind> = Object.fromEntries(
	await Promise.all(
		readdirSync(folder).map(async (file) => {
			const { default: kind }: { default: Kind } = await import(new URL(file, folder).href);

			return [`./kinds/${file}`, kind] as const;
		})
	)
);
