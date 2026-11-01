// The website's screenshots: the production build on its own local network, seeded with the demo
// accounts and projects (apps/web/scripts/dev-network.ts, which needs Docker for HappyView), on
// ports apart from `pnpm dev`'s, the end-to-end tests' and Lighthouse's so it can run beside them.
import { join } from 'node:path';
import { ROOT, run, start } from '../run.ts';
import type { Listing, Shot } from '../types.ts';

const PORT = 5196;

const ORIGIN = `http://127.0.0.1:${PORT}`;

const PAGES = [
	{ name: 'home', path: '/' },
	{ name: 'profile', path: '/projects/neon-tide' },
	{ name: 'mix', path: '/projects/neon-tide/mixes/1' },
	{
		name: 'match',
		path: '/match?go=1&place=Berlin&lat=52.52&lng=13.405&radius=50&genres=techno&budget=established&setType=peak'
	}
];

export const web: Listing = {
	id: 'web',
	name: 'Local Network',
	eyebrow: 'localnetwork.computer',
	icon: 'apps/web/src/lib/assets/favicon.svg',
	glow: '#3987e5',
	hero: { shots: [1], frame: { kind: 'browser', url: 'localnetwork.computer/projects/neon-tide' } },
	async capture(browser) {
		await run('pnpm', ['-C', 'apps/web', 'build']);

		const app = await start('pnpm', ['exec', 'tsx', 'scripts/dev-network.ts', '--app', '--production'], {
			cwd: join(ROOT, 'apps/web'),
			env: {
				PORT: String(PORT),
				DEV_PLC_PORT: '2982',
				DEV_PDS_PORT: '2983',
				DEV_DB_PATH: 'data/atstore.db',
				DEV_ENV_FILE: '',
				GEOCODER: 'none',
				LOG_LEVEL: 'warn'
			},
			// adapter-node prints this once the app has connected its root account and is serving.
			ready: /Listening on/
		});

		try {
			const page = await browser.newPage({
				viewport: { width: 1440, height: 900 },
				deviceScaleFactor: 2,
				colorScheme: 'dark'
			});

			const shots: Shot[] = [];

			for (const { name, path } of PAGES) {
				const response = await page.goto(`${ORIGIN}${path}`, { waitUntil: 'networkidle' });

				// An error page would be screenshotted as happily as the real one.
				if (!response?.ok()) throw new Error(`${path} answered ${response?.status()}`);

				await page.evaluate(() => document.fonts.ready);
				shots.push({ name, png: await page.screenshot({ type: 'png' }) });
			}

			return shots;
		} finally {
			await app.stop();
		}
	}
};
