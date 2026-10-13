/**
 * Boots a local, spaces-enabled AT Protocol network for development:
 * a PLC directory and an alpha PDS (from @atproto/dev-env), plus the
 * Local Network root account (also the lexicon authority), staff and demo
 * accounts, published lexicons, and demo projects, then a HappyView in Docker
 * (scripts/lib/happyview.ts) to index them. Writes `.env.local`, including
 * HAPPYVIEW_URL and HAPPYVIEW_API_KEY.
 *
 *   pnpm dev:network          # then, in another terminal: pnpm dev
 */
import { spawn } from 'node:child_process';
import { rmSync, writeFileSync } from 'node:fs';
import { asDid, AtpAgent } from '@atproto/api';
import { TID } from '@atproto/common-web';
import { mockNetworkUtilities, TestPds, TestPlc } from '@atproto/dev-env';
import { LEXICON_DOCS, publishLexicons, validateRecord } from '../src/lib/server/atproto/lexicons.ts';
import { COLLECTION } from '../src/lib/nsid.ts';
import { slugify } from '../src/lib/server/services/projects.ts';
import { startHappyView } from './lib/happyview.ts';
import { DEMO_PROJECTS, DEMO_SETS } from './seed-data.ts';

const PLC_PORT = Number(process.env.DEV_PLC_PORT ?? 2582);

const PDS_PORT = Number(process.env.DEV_PDS_PORT ?? 2583);

const DB_PATH = process.env.DEV_DB_PATH ?? 'data/dev.db';

const ENV_FILE = process.env.DEV_ENV_FILE ?? '.env.local';

/** With --app, also start the SvelteKit dev server wired to this network (used by Playwright). */
const START_APP = process.argv.includes('--app');

/** With --production as well, the app is the production build in build/ instead (used by Lighthouse). */
const PRODUCTION = process.argv.includes('--production');

const APP_PORT = Number(process.env.PORT ?? 5173);

const ROOT = { handle: 'localnetwork.test', email: 'root@localnetwork.test', password: 'root-pass' };

const ACCOUNTS = [
	{ handle: 'ada.test', password: 'ada-pass', note: 'admin (ADMIN_DIDS)' },
	{ handle: 'milo.test', password: 'milo-pass', note: 'regular account; make them a moderator in Staff' },
	{ handle: 'neon.test', password: 'neon-pass', note: 'DJ; manages two demo projects, has two sets' },
	{ handle: 'pat.test', password: 'pat-pass', note: 'promoter' }
];

const log = (message: string) => console.log(`\x1b[35m▸\x1b[0m ${message}`);

async function main() {
	rmSync(DB_PATH, { force: true });
	rmSync(`${DB_PATH}-wal`, { force: true });
	rmSync(`${DB_PATH}-shm`, { force: true });

	const plc = await TestPlc.create({ port: PLC_PORT });
	log(`PLC directory  ${plc.url}`);

	const pds = await TestPds.create({
		port: PDS_PORT,
		hostname: 'localhost',
		didPlcUrl: plc.url,
		enableDidDocWithSession: true,
		inviteRequired: false
	});

	mockNetworkUtilities([pds]);
	log(`PDS (spaces)   ${pds.url}`);

	const rootDid = (await pds.getAgent().com.atproto.server.createAccount(ROOT)).data.did;
	// The root account is the lexicon authority for computer.localnetwork.* (in
	// production that is a `_lexicon` DNS TXT record). The PDS reads this
	// setting on every resolution, so it can be pointed at the new account.
	pds.ctx.cfg.lexicon.didAuthority = asDid(rootDid);
	const root = { did: rootDid };

	const rootAgent = new AtpAgent({ service: pds.url });
	await rootAgent.login({ identifier: ROOT.handle, password: ROOT.password });
	await publishLexicons(rootAgent);
	log(`Published ${LEXICON_DOCS.length} lexicons from ${ROOT.handle}`);

	const dids: Record<string, string> = {};

	for (const account of ACCOUNTS) {
		const agent = pds.getAgent();

		const result = await agent.com.atproto.server.createAccount({
			handle: account.handle,
			email: `${account.handle}@example.test`,
			password: account.password
		});

		dids[account.handle] = result.data.did;
	}

	const now = new Date().toISOString();

	for (const [index, profile] of DEMO_PROJECTS.entries()) {
		const managers = index < 2 ? [dids['neon.test']] : [];

		const record = validateRecord(COLLECTION.project, {
			slug: slugify(profile.name),
			profile: JSON.parse(JSON.stringify(profile)),
			managers,
			status: 'active',
			revision: 1,
			createdBy: dids['ada.test'],
			createdAt: now,
			updatedAt: now
		});

		await rootAgent.com.atproto.repo.putRecord({
			repo: root.did,
			collection: COLLECTION.project,
			rkey: TID.nextStr(),
			record
		});
	}

	log(`Seeded ${DEMO_PROJECTS.length} demo projects`);

	// HappyView isn't running yet, so it reads these when neon first opens a set picker, as it would for older sets.
	const neon = new AtpAgent({ service: pds.url });
	await neon.login({
		identifier: 'neon.test',
		password: ACCOUNTS.find((account) => account.handle === 'neon.test')!.password
	});

	for (const set of DEMO_SETS)
		await neon.com.atproto.repo.createRecord({
			repo: neon.assertDid,
			collection: COLLECTION.djSet,
			record: validateRecord(COLLECTION.djSet, set)
		});

	log(`Seeded ${DEMO_SETS.length} sets for neon.test`);

	const happyView = await startHappyView({ plcUrl: plc.url, pdsUrl: pds.url });
	log(`HappyView      ${happyView.url}`);

	// No NODE_ENV: Vite applies it from .env files, so `pnpm build` would ship
	// Svelte's dev runtime. `vite dev` defaults to development on its own.
	const vars = {
		PUBLIC_URL: `http://127.0.0.1:${APP_PORT}`,
		DATABASE_PATH: DB_PATH,
		ROOT_IDENTIFIER: root.did,
		// The alpha PDS only mints space delegation tokens for full-access
		// sessions, so the root signs in with its account password, not an app password.
		ROOT_PASSWORD: ROOT.password,
		ROOT_SERVICE: pds.url,
		ADMIN_DIDS: dids['ada.test'],
		PLC_URL: plc.url,
		HANDLE_RESOLVER_URL: pds.url,
		HAPPYVIEW_URL: happyView.url,
		HAPPYVIEW_API_KEY: happyView.apiKey,
		SYNC_POLL_INTERVAL_MS: '10000',
		GEOCODER: process.env.GEOCODER ?? 'open-meteo',
		LOG_LEVEL: process.env.LOG_LEVEL ?? 'debug'
	};

	if (ENV_FILE) {
		const lines = Object.entries(vars).map(([key, value]) => `${key}=${value}`);
		writeFileSync(ENV_FILE, ['# Written by `pnpm dev:network`. Regenerated on every run.', ...lines, ''].join('\n'));
		log(`Wrote ${ENV_FILE}`);
	}

	console.log('\n  Accounts (sign in with the handle; the PDS asks for the password):');
	console.log(`    ${ROOT.handle.padEnd(20)} ${root.did}  root account`);

	for (const account of ACCOUNTS) {
		console.log(
			`    ${account.handle.padEnd(20)} ${dids[account.handle]}  ${account.password.padEnd(14)} ${account.note}`
		);
	}

	console.log(
		`\n  Now run \`pnpm dev\` and open http://127.0.0.1:${APP_PORT}\n  Ctrl-C stops the network (all data is in-memory/temporary).\n`
	);

	// The production build only reaches this network's loopback PDS and PLC with the SSRF guard off.
	const production = { NODE_ENV: 'production', HOST: '127.0.0.1', ALLOW_PRIVATE_NETWORK: 'true' };

	const app = START_APP
		? spawn(PRODUCTION ? process.execPath : 'pnpm', PRODUCTION ? ['build'] : ['exec', 'vite', 'dev'], {
				stdio: 'inherit',
				env: { ...process.env, ...vars, ...(PRODUCTION && production), PORT: String(APP_PORT) }
			})
		: undefined;

	let stopping = false;

	const shutdown = async () => {
		if (stopping) return;
		stopping = true;
		setTimeout(() => process.exit(0), 5_000).unref();
		app?.kill('SIGTERM');
		// HappyView first: stopping it also stops its firehose subscription before the PDS goes away.
		await Promise.allSettled([happyView.stop(), pds.close(), plc.close()]);
		process.exit(0);
	};

	process.on('SIGINT', () => void shutdown());
	process.on('SIGTERM', () => void shutdown());
}

main().catch((error) => {
	console.error(error);
	process.exit(1);
});
