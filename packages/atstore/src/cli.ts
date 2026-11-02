// `pnpm atstore [tool...]`: makes every tool's AT Store pictures, or only the named tools', into
// packages/atstore/out/<tool>/, and checks them against what the AT Store takes now. A tool with a
// page on the website gets every capture written to apps/web/src/lib/tools/captures/<tool>/ too.
import { readFileSync } from 'node:fs';
import { join, relative } from 'node:path';
import { imageSize } from 'image-size';
import { chromium } from 'playwright';
import { pick, save, screenshots, SITE } from './captures.ts';
import { fetchRules, LISTING, problems, type Asset } from './check.ts';
import { listings } from './listings/index.ts';
import { renderHero, renderIcon } from './render.ts';
import { ROOT } from './run.ts';
import type { Capture, Listing } from './types.ts';

const OUT = join(import.meta.dirname, '../out');

const log = (message: string) => console.log(`\x1b[35m▸\x1b[0m ${message}`);

async function make(
	listing: Listing,
	browser: Parameters<Listing['capture']>[0]
): Promise<{ assets: Asset[]; captures: Capture[] }> {
	log(`${listing.id}: screenshots`);
	const captures = await listing.capture(browser);
	const svg = readFileSync(join(ROOT, listing.icon), 'utf8');
	const heroShots = pick(captures, listing.hero.shots);

	log(`${listing.id}: icon and hero`);

	return {
		captures,
		assets: [
			{ file: 'icon.png', field: 'icon', png: await renderIcon(browser, svg) },
			{ file: 'hero.png', field: 'heroImage', png: await renderHero(browser, listing, svg, heroShots) },
			...screenshots(listing, captures)
		]
	};
}

/** Puts `files` in `folder`, in place of what was there, and lists them with their sizes. */
function write(folder: string, files: Capture[]) {
	save(folder, files);

	for (const { file, bytes } of files) {
		const { width, height } = file.endsWith('.png') ? imageSize(bytes) : {};
		const size = width ? `${width}x${height}` : '';
		console.log(`  ${file.padEnd(36)} ${size.padEnd(10)} ${(bytes.length / 1024).toFixed(0).padStart(6)} KiB`);
	}

	log(relative(process.cwd(), folder));
}

async function main() {
	const asked = process.argv.slice(2);
	const unknown = asked.filter((id) => !listings.some((listing) => listing.id === id));

	if (unknown.length > 0) {
		console.error(
			`No tool called ${unknown.join(', ')}. The tools: ${listings.map((listing) => listing.id).join(', ')}`
		);
		process.exit(2);
	}

	const chosen = asked.length > 0 ? listings.filter((listing) => asked.includes(listing.id)) : listings;
	// First, so a store that moved its lexicon stops the run before anything is built.
	const rules = await fetchRules();
	log(`${LISTING}: what the AT Store takes now`);
	const browser = await chromium.launch();
	let failed = false;

	try {
		for (const listing of chosen) {
			const { assets, captures } = await make(listing, browser);
			write(
				join(OUT, listing.id),
				assets.map(({ file, png }) => ({ file, bytes: png }))
			);
			const found = problems(rules, assets);

			for (const problem of found) console.error(`  \x1b[31m✗\x1b[0m ${problem}`);
			failed ||= found.length > 0;

			if (listing.page) write(join(SITE, listing.id), captures);
		}
	} finally {
		await browser.close();
	}

	if (failed) process.exit(1);
}

await main();
