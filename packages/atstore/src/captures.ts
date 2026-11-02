// What a tool's captures become: the store's screenshots, picked out by name, and the files its
// page on the website shows.
import { mkdirSync, rmSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import type { Asset } from './check.ts';
import { ROOT } from './run.ts';
import type { Capture, Listing } from './types.ts';

/** Where the website keeps each tool's captures, in a folder per tool. */
export const SITE = join(ROOT, 'apps/web/src/lib/tools/captures');

/** The screenshots called `names` (`player` is `player.png`), in that order. */
export function pick(captures: Capture[], names: string[]): Buffer[] {
	return names.map((name) => {
		const found = captures.find((capture) => capture.file === `${name}.png`);

		if (!found) throw new Error(`no ${name}.png among ${captures.map((capture) => capture.file).join(', ')}`);

		return found.bytes;
	});
}

/** The listing's screenshots as the store takes them: `screenshot-1-player.png` and on. */
export function screenshots(listing: Listing, captures: Capture[]): Asset[] {
	return pick(captures, listing.screenshots).map((png, index) => ({
		file: `screenshot-${index + 1}-${listing.screenshots[index]}.png`,
		field: 'screenshots',
		png
	}));
}

/** Writes `files` into `folder`, in place of whatever it held. */
export function save(folder: string, files: Capture[]): void {
	rmSync(folder, { recursive: true, force: true });
	mkdirSync(folder, { recursive: true });

	for (const { file, bytes } of files) writeFileSync(join(folder, file), bytes);
}
