// Lighthouse CI (`pnpm test:lighthouse`, which builds first). It measures the production build on the
// same local network and seeded projects as the end-to-end tests (scripts/dev-network.ts), so the
// pages have real data in them. Ports apart from the e2e run's, so the two can run side by side.
// The pages are the ones lighthouse/pages.json marks `pr`; scripts/lighthouse.ts measures all of them
// weekly, on phones and desktops, and tracks each against its history.
const { chromium } = require('playwright');

const { pages } = require('./lighthouse/pages.json');

const PORT = 5198;

const timing = process.env.LIGHTHOUSE_TIMING !== 'false';

const env = {
	PORT,
	DEV_PLC_PORT: 2782,
	DEV_PDS_PORT: 2783,
	DEV_DB_PATH: 'data/lighthouse.db',
	DEV_ENV_FILE: '',
	GEOCODER: 'none',
	ACTOR_SEARCH: 'none',
	LOG_LEVEL: 'warn'
};

module.exports = {
	ci: {
		collect: {
			startServerCommand: `${Object.entries(env)
				.map(([key, value]) => `${key}=${value}`)
				.join(' ')} tsx scripts/dev-network.ts --app --production`,
			// adapter-node prints this once the app has connected its root account and is serving.
			startServerReadyPattern: 'Listening on',
			startServerReadyTimeout: 180_000,
			url: pages.filter((page) => page.pr).map(({ path }) => `http://127.0.0.1:${PORT}${path}`),
			numberOfRuns: 5,
			// Playwright's Chromium, which the e2e tests install anyway, rather than whichever Chrome the
			// machine has (or doesn't).
			chromePath: chromium.executablePath()
		},
		// Floors and budgets just under what the three pages measured locally over four runs
		// (performance 97 to 99, LCP 1.7 to 2.0 s, no blocking time or layout shift). Timing is taken on the
		// median of each page's five runs, each metric's own, as Lighthouse's guide on variability
		// suggests. Layout shift is taken on the worst run: it isn't noise, and a shift on one load (a
		// cold cache's) is one a visitor sees. The other categories are all or nothing: each was 100. A
		// merge queue group (LIGHTHOUSE_TIMING=false) checks only what doesn't depend on the machine's
		// speed, so a slow runner can't hold up the queue.
		assert: {
			assertions: {
				...(timing && {
					'categories:performance': ['error', { minScore: 0.95, aggregationMethod: 'median' }],
					'largest-contentful-paint': ['error', { maxNumericValue: 2_300, aggregationMethod: 'median' }],
					'total-blocking-time': ['error', { maxNumericValue: 100, aggregationMethod: 'median' }]
				}),
				'categories:accessibility': ['error', { minScore: 1 }],
				'categories:best-practices': ['error', { minScore: 1 }],
				'categories:seo': ['error', { minScore: 1 }],
				'cumulative-layout-shift': ['error', { maxNumericValue: 0.02, aggregationMethod: 'pessimistic' }]
			}
		},
		// Reports stay on the machine (CI uploads them as an artifact), never on a public server.
		upload: { target: 'filesystem', outputDir: 'lighthouse-report' }
	}
};
