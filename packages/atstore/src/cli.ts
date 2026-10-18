// `pnpm atstore [tool...]`: makes every tool's AT Store pictures, or only the named tools', into
// packages/atstore/out/<tool>/, and checks them against what the AT Store takes now.
import { mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { join, relative } from 'node:path';
import { imageSize } from 'image-size';
import { chromium } from 'playwright';
import { fetchRules, LISTING, problems, type Asset } from './check.ts';
import { listings } from './listings/index.ts';
import { renderHero, renderIcon } from './render.ts';
import { ROOT } from './run.ts';
import type { Listing } from './types.ts';

const OUT = join(import.meta.dirname, '../out');

const log = (message: string) => console.log(`\x1b[35m▸\x1b[0m ${message}`);

async function make(listing: Listing, browser: Parameters<Listing['capture']>[0]): Promise<Asset[]> {
	log(`${listing.id}: screenshots`);
	const shots = await listing.capture(browser);
	const svg = readFileSync(join(ROOT, listing.icon), 'utf8');
	const heroShots = listing.hero.shots.map((index) => shots[index]).filter((shot) => shot !== undefined);

	log(`${listing.id}: icon and hero`);

	return [
		{ file: 'icon.png', field: 'icon', png: await renderIcon(browser, svg) },
		{ file: 'hero.png', field: 'heroImage', png: await renderHero(browser, listing, svg, heroShots) },
		...shots.map((shot, index): Asset => ({
			file: `screenshot-${index + 1}-${shot.name}.png`,
			field: 'screenshots',
			png: shot.png
		}))
	];
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
			const assets = await make(listing, browser);
			const folder = join(OUT, listing.id);
			rmSync(folder, { recursive: true, force: true });
			mkdirSync(folder, { recursive: true });

			for (const { file, png } of assets) {
				writeFileSync(join(folder, file), png);
				const { width, height } = imageSize(png);
				console.log(
					`  ${file.padEnd(36)} ${`${width}x${height}`.padEnd(10)} ${(png.length / 1024).toFixed(0).padStart(6)} KiB`
				);
			}

			const found = problems(rules, assets);

			for (const problem of found) console.error(`  \x1b[31m✗\x1b[0m ${problem}`);
			failed ||= found.length > 0;
			log(`${listing.id}: ${relative(process.cwd(), folder)}`);
		}
	} finally {
		await browser.close();
	}

	if (failed) process.exit(1);
}

await main();
