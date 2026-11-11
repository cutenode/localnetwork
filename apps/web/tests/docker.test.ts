// The image as it's deployed (`pnpm test:docker`, once `docker build -f apps/web/Dockerfile -t
// localnetwork .` from the repo root has built it): an empty volume at /data and its configuration
// in the environment, against the local network `pnpm dev:network` starts (the one the e2e tests
// use).
import { type ChildProcess, spawn } from 'node:child_process';
import { once } from 'node:events';
import { mkdtempSync, openSync, readFileSync, rmSync, statSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { setTimeout as sleep } from 'node:timers/promises';
import { afterAll, beforeAll, beforeEach, expect, it, vi } from 'vitest';
import { checkDocker, docker } from '../scripts/lib/happyview.ts';
import headline from '../src/lib/headline.json' with { type: 'json' };

const ROOT = resolve(import.meta.dirname, '..');

const IMAGE = 'localnetwork';

const NAME = `localnetwork-docker-test-${process.pid}`;

const APP = 'http://127.0.0.1:3000';

const work = mkdtempSync(join(tmpdir(), 'localnetwork-docker-'));

let network: ChildProcess | undefined;

beforeAll(async () => {
	await checkDocker();

	// The network writes the app's configuration to DEV_ENV_FILE once its accounts, projects and
	// HappyView are ready.
	const env = join(work, 'env');
	const log = join(work, 'network.log');
	const output = openSync(log, 'w');
	network = spawn(join(ROOT, 'node_modules/.bin/tsx'), ['scripts/dev-network.ts'], {
		cwd: ROOT,
		env: {
			...process.env,
			PORT: '3000',
			DEV_PLC_PORT: '2882',
			DEV_PDS_PORT: '2883',
			DEV_DB_PATH: join(work, 'network.db'),
			DEV_ENV_FILE: env
		},
		stdio: ['ignore', output, output]
	});

	for (let waited = 0; !statSync(env, { throwIfNoEntry: false })?.size; waited++) {
		if (network.exitCode !== null) throw new Error(`the network exited:\n${readFileSync(log, 'utf8')}`);

		if (waited === 180) throw new Error("the network didn't start in 3 minutes");
		await sleep(1000);
	}

	// Host networking, so the app reaches the network on 127.0.0.1 as its configuration says. The
	// database goes on the volume, as in production; the SSRF guard has to allow the loopback PDS.
	await docker(
		'run',
		'--detach',
		'--name',
		NAME,
		'--network',
		'host',
		'--env-file',
		env,
		'--env',
		'DATABASE_PATH=/data/localnetwork.db',
		'--env',
		'ALLOW_PRIVATE_NETWORK=true',
		'--mount',
		'type=volume,target=/data',
		IMAGE
	);

	const answers = async () => {
		const response = await fetch(`${APP}/healthz`);

		if (!response.ok) throw new Error(`/healthz answered ${response.status}`);
	};

	try {
		await vi.waitFor(answers, { timeout: 60_000, interval: 1000 });
	} catch {
		throw new Error(`the image didn't answer in a minute:\n${await docker('logs', NAME)}`);
	}
}, 300_000);

afterAll(async () => {
	await docker('rm', '--force', '--volumes', NAME).catch(() => undefined);

	if (network && network.exitCode === null) {
		network.kill('SIGTERM');
		await once(network, 'exit');
	}

	rmSync(work, { recursive: true, force: true });
});

beforeEach(({ onTestFailed }) => {
	onTestFailed(async () => console.error(`--- container logs\n${await docker('logs', NAME)}`));
});

it.each([
	['/healthz', '"happyView":"ok"'],
	// The headline's first pair, which the server renders.
	['/', `${headline.verbs[0]} your ${headline.nouns[0]}.`],
	['/projects/halflight', 'Halflight'],
	// The tools' guides come from apps/<tool>/, which only the image's build context brings in.
	['/tools/locallibrary/docs/install', 'Installing locallibrary']
])('answers %s with 200 and the page', async (path, text) => {
	const response = await fetch(`${APP}${path}`, { redirect: 'manual' });

	expect(response.status).toBe(200);
	expect(await response.text()).toContain(text);
});

// The image runs scripts/db-backup.ts from source. With no bucket set it stops at asking for one,
// which it only gets to once every file it imports is in the image.
it('runs scripts/db-backup.ts', async () => {
	const output = await docker('exec', NAME, 'node', 'scripts/db-backup.ts', 'list').catch(
		(error: Error) => error.message
	);

	expect(output).toContain('Set BACKUP_S3_BUCKET');
});
