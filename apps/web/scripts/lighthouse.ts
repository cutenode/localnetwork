/**
 * Lighthouse CI on every page in lighthouse/pages.json, on a phone and a desktop, signed in where a page
 * needs it, then each page's runs summed up for scripts/lighthouse-history.ts (`pnpm test:lighthouse:full`
 * builds first, then records the run in the history). Uses lighthouserc.cjs's network, Chromium and
 * budgets, and starts the network once for every page.
 *
 *   tsx scripts/lighthouse.ts                          # every page, both form factors
 *   tsx scripts/lighthouse.ts --form-factor desktop    # one form factor (CI runs one per job)
 *   tsx scripts/lighthouse.ts --page /charts --runs 5  # pages whose path contains /charts, five runs each
 *
 * Writes lighthouse-full/summary-<form factor>.json, and each run's report under lighthouse-full/<form factor>/.
 */
import { spawn, spawnSync } from 'node:child_process';
import { once } from 'node:events';
import { mkdirSync, readdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { createRequire } from 'node:module';
import { join } from 'node:path';
import { parseArgs } from 'node:util';
import { chromium } from 'playwright';
import { signIn } from '../tests/sign-in.ts';
import { type Account, type FormFactor, type Lhr, median, summarizePage, type Summary } from './lib/lighthouse.ts';

type Page = { path: string; account?: Account; pr?: boolean };

const require = createRequire(import.meta.url);

const { ci } = require('../lighthouserc.cjs');

const { pages }: { pages: Page[] } = require('../lighthouse/pages.json');

// The seeded accounts of scripts/dev-network.ts.
const ACCOUNTS: Record<Account, { handle: string; password: string }> = {
	dj: { handle: 'neon.test', password: 'neon-pass' },
	admin: { handle: 'ada.test', password: 'ada-pass' }
};

const OUT = 'lighthouse-full';

const LHCI_DIR = '.lighthouseci';

const { values: args } = parseArgs({
	options: {
		'form-factor': { type: 'string', multiple: true, default: ['mobile', 'desktop'] },
		page: { type: 'string', multiple: true },
		runs: { type: 'string', default: String(ci.collect.numberOfRuns) }
	}
});

const formFactors = args['form-factor'].filter(
	(value): value is FormFactor => value === 'mobile' || value === 'desktop'
);

const selected = pages.filter(({ path }) => !args.page || args.page.some((filter) => path.includes(filter)));

const origin = new URL(ci.collect.url[0]).origin;

if (!selected.length || formFactors.length !== args['form-factor'].length) {
	console.error(
		'Nothing to measure: check --page (a part of a path in lighthouse/pages.json) and --form-factor (mobile or desktop).'
	);
	process.exit(1);
}

const log = (message: string) => console.log(`\x1b[35m▸\x1b[0m ${message}`);

/** The network and the production build, as lhci's startServerCommand would start them; resolves once it serves. */
async function startServer() {
	const server = spawn(ci.collect.startServerCommand, {
		shell: true,
		detached: true,
		stdio: ['ignore', 'pipe', 'inherit']
	});

	const exited = once(server, 'exit').then(([code]) => {
		throw new Error(`The network exited (${code}) before the app was serving.`);
	});

	const ready = new Promise<void>((resolve) => {
		let output = '';
		server.stdout.on('data', (chunk: Buffer) => {
			process.stdout.write(chunk);
			output += chunk.toString();

			if (output.includes(ci.collect.startServerReadyPattern)) resolve();
		});
	});

	const timeout = new Promise<never>((_, reject) =>
		setTimeout(
			() => reject(new Error('The network took too long to start.')),
			ci.collect.startServerReadyTimeout
		).unref()
	);

	await Promise.race([ready, exited, timeout]);

	return async () => {
		// The whole group, so the shell, tsx and the network all get it; the network then stops the
		// app and removes HappyView's container (scripts/dev-network.ts).
		if (server.exitCode !== null || !server.pid) return;
		const exit = once(server, 'exit');
		process.kill(-server.pid, 'SIGTERM');
		await exit;
	};
}

/** A Cookie header for each account the selected pages are measured as, from a real sign-in. */
async function sessionCookies() {
	const accounts = [...new Set(selected.map((page) => page.account).filter((account) => account !== undefined))];
	const browser = await chromium.launch();
	const cookies: Partial<Record<Account, string>> = {};

	try {
		for (const account of accounts) {
			// The production build's origin is https without this header (scripts/dev-network.ts).
			const context = await browser.newContext({ baseURL: origin, extraHTTPHeaders: { 'X-Forwarded-Proto': 'http' } });
			await signIn(await context.newPage(), ACCOUNTS[account].handle, ACCOUNTS[account].password);
			cookies[account] = (await context.cookies(origin)).map(({ name, value }) => `${name}=${value}`).join('; ');
			await context.close();
		}
	} finally {
		await browser.close();
	}

	return cookies;
}

const lhci = (command: string, config: string, ...flags: string[]) =>
	spawnSync('lhci', [command, `--config=${config}`, ...flags], { stdio: 'inherit' });

/** Each failed lhci assertion of the last `lhci assert`, by page path. */
function assertionFailures() {
	const failures = new Map<string, string[]>();

	let results: {
		url: string;
		level: string;
		auditId: string;
		auditProperty?: string;
		actual: number;
		operator: string;
		expected: number;
	}[];

	try {
		results = JSON.parse(readFileSync(join(LHCI_DIR, 'assertion-results.json'), 'utf8'));
	} catch {
		return failures;
	}

	for (const result of results.filter(({ level }) => level === 'error')) {
		const url = new URL(result.url);
		const audit = result.auditProperty ? `${result.auditId}.${result.auditProperty}` : result.auditId;
		const failure = `failed ${audit}: ${Math.round(result.actual * 100) / 100}, expected ${result.operator} ${result.expected}`;
		failures.set(url.pathname + url.search, [...(failures.get(url.pathname + url.search) ?? []), failure]);
	}

	return failures;
}

/** The results of the last `lhci collect`, by the page path each run asked for. */
function collectedLhrs() {
	const lhrs = new Map<string, Lhr[]>();

	for (const name of readdirSync(LHCI_DIR).filter((file) => file.startsWith('lhr-') && file.endsWith('.json'))) {
		const lhr: Lhr & { requestedUrl: string } = JSON.parse(readFileSync(join(LHCI_DIR, name), 'utf8'));
		const url = new URL(lhr.requestedUrl);
		lhrs.set(url.pathname + url.search, [...(lhrs.get(url.pathname + url.search) ?? []), lhr]);
	}

	return lhrs;
}

// Whole, so a summary from an earlier run of another form factor isn't compared or recorded again.
rmSync(OUT, { recursive: true, force: true });

mkdirSync(join(OUT, 'config'), { recursive: true });

const stopServer = await startServer();

try {
	const cookies = await sessionCookies();

	for (const formFactor of formFactors) {
		const summary: Summary = {
			formFactor,
			lighthouseVersion: '',
			benchmarkIndex: 0,
			runs: Number(args.runs),
			pages: []
		};

		const benchmarks: number[] = [];

		for (const account of [undefined, 'dj', 'admin'] satisfies (Account | undefined)[]) {
			const group = selected.filter((page) => page.account === account);

			if (!group.length) continue;

			const name = `${formFactor}-${account ?? 'signed-out'}`;
			const config = join(OUT, 'config', `${name}.json`);
			const cookie = account && cookies[account];
			log(
				`${group.length} page${group.length === 1 ? '' : 's'}, ${formFactor}, ${account ? `signed in as ${ACCOUNTS[account].handle}` : 'signed out'}`
			);

			writeFileSync(
				config,
				JSON.stringify({
					ci: {
						collect: {
							numberOfRuns: Number(args.runs),
							chromePath: ci.collect.chromePath,
							settings: {
								...(formFactor === 'desktop' && { preset: 'desktop' }),
								...(cookie && { extraHeaders: { Cookie: cookie } })
							}
						},
						// Search engines never see a signed-in page, so its SEO score is tracked but not held to a budget.
						assert: {
							assertions: Object.fromEntries(
								Object.entries(ci.assert.assertions).filter(([id]) => !account || id !== 'categories:seo')
							)
						},
						upload: { target: 'filesystem', outputDir: join(OUT, formFactor, account ?? 'signed-out') }
					}
				})
			);

			// One page per collect, adding to the last: lhci gives up on the pages after one that fails to
			// load, and here that page just has no runs in its summary.
			for (const [index, page] of group.entries()) {
				lhci('collect', config, `--url=${origin}${page.path}`, ...(index ? ['--additive'] : []));
			}

			lhci('assert', config);
			lhci('upload', config);

			const lhrs = collectedLhrs();
			const failures = assertionFailures();

			for (const page of group) {
				const runs = lhrs.get(page.path) ?? [];
				summary.lighthouseVersion ||= runs[0]?.lighthouseVersion ?? '';
				benchmarks.push(...runs.map((lhr) => lhr.environment.benchmarkIndex));
				summary.pages.push(summarizePage({ path: page.path, account: page.account }, runs, failures.get(page.path)));
			}
		}

		summary.benchmarkIndex = benchmarks.length ? median(benchmarks) : 0;
		writeFileSync(join(OUT, `summary-${formFactor}.json`), `${JSON.stringify(summary, null, '\t')}\n`);
		log(`${OUT}/summary-${formFactor}.json`);
	}
} finally {
	await stopServer();
}
