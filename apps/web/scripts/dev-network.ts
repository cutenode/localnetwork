/**
 * Boots a local, spaces-enabled AT Protocol network for development:
 * a PLC directory and an alpha PDS (from @atproto/dev-env), plus the
 * Local Network root account (also the lexicon authority), staff and demo
 * accounts, published lexicons, and demo projects, then a HappyView in Docker
 * (scripts/lib/happyview.ts) to index them. Writes `.env.local`, including
 * HAPPYVIEW_URL and HAPPYVIEW_API_KEY.
 *
 *   pnpm dev                  # the network and the app
 *   pnpm dev:network          # the network only; then, in another terminal: pnpm dev:app
 */
import { spawn } from 'node:child_process';
import { readFileSync, rmSync, writeFileSync } from 'node:fs';
import { setTimeout as sleep } from 'node:timers/promises';
import { asDid, AtpAgent } from '@atproto/api';
import { TID } from '@atproto/common-web';
import { mockNetworkUtilities, TestPds, TestPlc } from '@atproto/dev-env';
import { USERINPUT } from '../src/lib/bug-report.ts';
import { LEXICON_DOCS, publishLexicons, validateRecord } from '../src/lib/server/atproto/lexicons.ts';
import { AgentRepoClient } from '../src/lib/server/atproto/repo-client.ts';
import { BSKY_PROFILE, CALENDAR_EVENT, COLLECTION, EPK_CONTENT_KEY, SPACE_TYPE } from '../src/lib/nsid.ts';
import { sampleReport } from '../src/lib/mix/sample.ts';
import { HappyViewClient } from '../src/lib/server/happyview/client.ts';
import { slugify } from '../src/lib/server/services/projects.ts';
import { eventRecord, showRecord } from '../src/lib/server/services/shows.ts';
import { checkDocker, startHappyView } from './lib/happyview.ts';
import {
	DEMO_DJ_PROFILE,
	DEMO_DJ_SETS,
	DEMO_DJS,
	DEMO_EPKS,
	DEMO_FOLLOWS,
	DEMO_PROJECTS,
	DEMO_SHOWS
} from './seed-data.ts';

const PLC_PORT = Number(process.env.DEV_PLC_PORT ?? 2582);

const PDS_PORT = Number(process.env.DEV_PDS_PORT ?? 2583);

const DB_PATH = process.env.DEV_DB_PATH ?? 'data/dev.db';

const ENV_FILE = process.env.DEV_ENV_FILE ?? '.env.local';

/** With --app, also start the SvelteKit dev server wired to this network (`pnpm dev`, and Playwright). */
const START_APP = process.argv.includes('--app');

/** With --production as well, the app is the production build in build/ instead (used by Lighthouse). */
const PRODUCTION = process.argv.includes('--production');

const APP_PORT = Number(process.env.PORT ?? 5173);

const ROOT = { handle: 'localnetwork.test', email: 'root@localnetwork.test', password: 'root-pass' };

const ACCOUNTS = [
	{ handle: 'ada.test', password: 'ada-pass', displayName: 'Ada', note: 'admin (ADMIN_DIDS)' },
	{
		handle: 'milo.test',
		password: 'milo-pass',
		displayName: 'Milo',
		note: 'regular account; make them a moderator in Staff'
	},
	{
		handle: 'neon.test',
		password: 'neon-pass',
		displayName: 'Neon',
		note: 'DJ; manages two demo projects, has three sets, two press kits, and four shows'
	},
	{ handle: 'pat.test', password: 'pat-pass', displayName: 'Pat', note: "promoter; follows both of neon's projects" },
	...DEMO_DJS.map((dj) => ({
		handle: dj.handle,
		password: dj.password,
		displayName: DEMO_PROJECTS[dj.project].name,
		note: `DJ; manages ${DEMO_PROJECTS[dj.project].name}, has ${dj.sets.length} sets`
	}))
];

const log = (message: string) => console.log(`\x1b[35m▸\x1b[0m ${message}`);

async function main() {
	// The PDS's better-sqlite3 is built for the Node that ran `pnpm install`, which engineStrict holds to
	// .node-version, but nothing checks the Node a script runs on, so another one crashes deep in the PDS.
	const nodeVersion = readFileSync(new URL('../../../.node-version', import.meta.url), 'utf8').trim();
	const isNodeVersion = process.versions.node.split('.')[0] === nodeVersion.split('.')[0];

	if (!isNodeVersion) {
		console.error(
			`This repo runs on Node.js ${nodeVersion} (.node-version), not ${process.version}. Run \`fnm use\`, then try again.`
		);
		process.exit(1);
	}

	// First, so a missing Docker fails right away with what to do instead of after the seeding.
	await checkDocker().catch((error) => {
		console.error(error instanceof Error ? error.message : error);
		process.exit(1);
	});

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

	// A stand-in for Local Network's board on userinput.app, so signed-in bug reports post here.
	const { data: userinputSpace } = await rootAgent.com.atproto.repo.createRecord({
		repo: rootDid,
		collection: USERINPUT.space,
		record: {
			$type: USERINPUT.space,
			name: 'Local Network',
			tags: [
				{ value: 'bug', label: 'Bug' },
				{ value: 'web', label: 'Web' }
			],
			createdAt: new Date().toISOString()
		}
	});

	const dids: Record<string, string> = {};

	for (const account of ACCOUNTS) {
		const agent = new AtpAgent({ service: pds.url });

		await agent.createAccount({
			handle: account.handle,
			email: `${account.handle}@example.test`,
			password: account.password
		});

		dids[account.handle] = agent.assertDid;
		// A Bluesky display name, which verifying an account in Staff → Verifications needs.
		await agent.com.atproto.repo.putRecord({
			repo: agent.assertDid,
			collection: BSKY_PROFILE,
			rkey: 'self',
			record: { $type: BSKY_PROFILE, displayName: account.displayName }
		});
	}

	// neon.test's own records, as locallibrary would publish them: playing stats and recent sets.
	const neon = new AtpAgent({ service: pds.url });
	await neon.login({ identifier: 'neon.test', password: 'neon-pass' });
	await neon.com.atproto.repo.putRecord({
		repo: dids['neon.test'],
		collection: 'computer.localnetwork.dj.profile',
		rkey: 'self',
		record: DEMO_DJ_PROFILE
	});

	// HappyView isn't running yet, so it reads these sets through backfillSeeds below, and again when the app first
	// syncs its scripts, as it would for older ones.
	const djs: { handle: string; agent: AtpAgent; sets: object[] }[] = [
		{ handle: 'neon.test', agent: neon, sets: DEMO_DJ_SETS }
	];

	for (const dj of DEMO_DJS) {
		const agent = new AtpAgent({ service: pds.url });
		await agent.login({ identifier: dj.handle, password: dj.password });
		djs.push({ handle: dj.handle, agent, sets: dj.sets });
	}

	for (const dj of djs) {
		for (const set of dj.sets) {
			await dj.agent.com.atproto.repo.createRecord({
				repo: dids[dj.handle],
				collection: COLLECTION.djSet,
				record: validateRecord(COLLECTION.djSet, { ...set, createdAt: new Date().toISOString() })
			});
		}
	}

	log(`Seeded neon.test's dj.profile and ${djs.reduce((sum, dj) => sum + dj.sets.length, 0)} dj.set records`);

	// A tracklist on Neon Tide's first mix, so its mix page has something to show: the sample report,
	// stored in the root's repo the way an approved request leaves it.
	const { data: report } = await rootAgent.com.atproto.repo.uploadBlob(
		new TextEncoder().encode(JSON.stringify(sampleReport)),
		{ encoding: 'application/json' }
	);

	const tracklist = {
		$type: 'blob',
		ref: { $link: report.blob.ref.toString() },
		mimeType: report.blob.mimeType,
		size: report.blob.size
	};

	const now = new Date().toISOString();
	const projectUris: string[] = [];

	for (const [index, profile] of DEMO_PROJECTS.entries()) {
		const managers = [
			...(index < 2 ? [dids['neon.test']] : []),
			...DEMO_DJS.filter((dj) => dj.project === index).map((dj) => dids[dj.handle])
		];

		const mixes = index === 0 ? profile.mixes?.map((mix, i) => (i === 0 ? { ...mix, tracklist } : mix)) : profile.mixes;

		const record = validateRecord(COLLECTION.project, {
			slug: slugify(profile.name),
			profile: JSON.parse(JSON.stringify({ ...profile, mixes })),
			managers,
			status: 'active',
			revision: 1,
			createdBy: dids['ada.test'],
			createdAt: now,
			updatedAt: now
		});

		const { data: written } = await rootAgent.com.atproto.repo.putRecord({
			repo: root.did,
			collection: COLLECTION.project,
			rkey: TID.nextStr(),
			record
		});

		projectUris.push(written.uri);
	}

	log(`Seeded ${DEMO_PROJECTS.length} demo projects`);

	// neon.test's press kits for Neon Tide, written by the same client the site uses: a space per
	// press kit with the root as a member, the private part in it, then the public record.
	const neonClient = new AgentRepoClient(dids['neon.test'], neon);

	for (const epk of DEMO_EPKS) {
		const rkey = TID.nextStr();

		const space = await neonClient.createSpace({
			spaceType: SPACE_TYPE.epk,
			skey: rkey,
			readPolicy: { $type: 'com.atproto.simplespace.defs#memberListPolicy' },
			writePolicy: { $type: 'com.atproto.simplespace.defs#publicPolicy' },
			appAccess: { $type: 'com.atproto.simplespace.defs#open' }
		});

		await neonClient.putMember({ space, did: root.did, read: true, write: true });
		await neonClient.putSpaceRecord({
			space,
			collection: COLLECTION.epkContent,
			rkey: EPK_CONTENT_KEY,
			record: validateRecord(COLLECTION.epkContent, { ...epk.content, updatedAt: now })
		});
		await neonClient.putRecord({
			collection: COLLECTION.epk,
			rkey,
			record: validateRecord(COLLECTION.epk, {
				project: projectUris[0],
				title: epk.title,
				headline: epk.headline,
				bio: epk.bio,
				mixes: epk.publicMixes.map((index) => DEMO_PROJECTS[0].mixes![index]),
				access: epk.access,
				createdAt: now,
				updatedAt: now
			})
		});
	}

	log(`Seeded ${DEMO_EPKS.length} press kits for ${DEMO_PROJECTS[0].name}`);

	// neon.test's shows, written the way the announce form writes them: the calendar event, then the show copying it.
	for (const { project, days, show } of DEMO_SHOWS) {
		const input = {
			...show,
			date: new Date(Date.now() + days * 86_400_000).toISOString().slice(0, 10),
			cancelled: !!show.cancelled
		};

		const rkey = TID.nextStr();
		const event = validateRecord(CALENDAR_EVENT, eventRecord(input));
		const ref = await neonClient.putRecord({ collection: CALENDAR_EVENT, rkey, record: event });
		const record = JSON.parse(JSON.stringify(showRecord(projectUris[project], ref, input, event)));
		await neonClient.putRecord({ collection: COLLECTION.show, rkey, record: validateRecord(COLLECTION.show, record) });
	}

	for (const { handle, project } of DEMO_FOLLOWS) {
		const agent = new AtpAgent({ service: pds.url });
		await agent.login({
			identifier: handle,
			password: ACCOUNTS.find((account) => account.handle === handle)!.password
		});
		await agent.com.atproto.repo.createRecord({
			repo: dids[handle],
			collection: COLLECTION.follow,
			record: validateRecord(COLLECTION.follow, { subject: projectUris[project], createdAt: now })
		});
	}

	log(`Seeded ${DEMO_SHOWS.length} shows and ${DEMO_FOLLOWS.length} follows`);

	const happyView = await startHappyView({ plcUrl: plc.url, pdsUrl: pds.url });
	log(`HappyView      ${happyView.url}`);
	void backfillSeeds(new HappyViewClient(happyView), projectUris, [
		{ collection: COLLECTION.djProfile, did: dids['neon.test'] },
		...djs.map(({ handle }) => ({ collection: COLLECTION.djSet, did: dids[handle] })),
		{ collection: COLLECTION.epk, did: dids['neon.test'] },
		{ collection: COLLECTION.show, did: dids['neon.test'] },
		{ collection: CALENDAR_EVENT, did: dids['neon.test'] },
		...DEMO_FOLLOWS.map(({ handle }) => ({ collection: COLLECTION.follow, did: dids[handle] }))
	]);

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
		USERINPUT_SPACE: userinputSpace.uri,
		SYNC_POLL_INTERVAL_MS: '10000',
		GEOCODER: process.env.GEOCODER ?? 'open-meteo',
		LOG_LEVEL: process.env.LOG_LEVEL ?? 'debug'
	};

	if (ENV_FILE) {
		const lines = Object.entries(vars).map(([key, value]) => `${key}=${value}`);
		writeFileSync(
			ENV_FILE,
			['# Written by `pnpm dev` and `pnpm dev:network`. Regenerated on every run.', ...lines, ''].join('\n')
		);
		log(`Wrote ${ENV_FILE}`);
	}

	console.log('\n  Accounts (sign in with the handle; the PDS asks for the password):');
	console.log(`    ${ROOT.handle.padEnd(20)} ${root.did}  root account`);

	for (const account of ACCOUNTS) {
		console.log(
			`    ${account.handle.padEnd(20)} ${dids[account.handle]}  ${account.password.padEnd(14)} ${account.note}`
		);
	}

	const next = START_APP ? 'Open' : 'Now run `pnpm dev:app` and open';

	console.log(
		`\n  ${next} http://127.0.0.1:${APP_PORT}\n  Ctrl-C stops the network (all data is in-memory/temporary).\n`
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

/**
 * HappyView hears of the seeded DJ records, press kits, shows, calendar events and follows only through a backfill,
 * and keeps them only once the app has uploaded their lexicons and scripts and HappyView has the projects they name or
 * their authors manage, which happens when the app boots (maybe later, with `pnpm dev:app`). So this waits for that in
 * the background, then has HappyView read them, DJ records first since project pages show them.
 */
async function backfillSeeds(
	client: HappyViewClient,
	projects: string[],
	repos: { collection: string; did: string }[]
) {
	const ready = async () =>
		(await client.lexicon(COLLECTION.follow)) !== null &&
		(await client.query('getProjects', { uris: projects })).projects.length === projects.length;

	while (!(await ready().catch(() => false))) await sleep(1_000);

	for (const { collection, did } of new Map(repos.map((repo) => [`${repo.collection} ${repo.did}`, repo])).values())
		await client.awaitBackfill(await client.backfill(collection, did), 30_000);
	log('HappyView has the demo DJ records, press kits, shows, calendar events and follows');
}

main().catch((error) => {
	console.error(error);
	process.exit(1);
});
