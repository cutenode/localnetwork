import { execFile, execFileSync } from 'node:child_process';
import { createHmac, randomBytes } from 'node:crypto';
import { once } from 'node:events';
import { mkdir, readFile, writeFile } from 'node:fs/promises';
import { type AddressInfo, createServer } from 'node:net';
import { join } from 'node:path';
import { setTimeout as sleep } from 'node:timers/promises';
import { fileURLToPath } from 'node:url';
import { promisify } from 'node:util';
import { isCidForBytes, parseCid } from '@atproto/lex-data';
import { z } from 'zod';
import { HAPPYVIEW_PERMISSIONS } from '../../src/lib/server/happyview/client.ts';
import { startJetstream } from './jetstream.ts';
import { startPlcProxy } from './plc-proxy.ts';

/** Also pulled by the jobs that start it in .github/workflows/web-ci.yml and web-scheduled.yml. */
const IMAGE = 'ghcr.io/gamesgamesgamesgamesgames/happyview:3.0.0-beta.14';

/**
 * HappyView's database, as in production, whose dialect the scripts' SQL is written in. v3 needs Postgres 17 or later.
 * CI pulls it beside IMAGE.
 */
const POSTGRES_IMAGE = 'postgres:17.11-alpine';

/**
 * The plugins the scripts in lua/ need, pinned like IMAGE: the Lua interpreter and the `happyview.sql`,
 * `happyview.db` and `happyview.http` libraries at 1.0.0-beta.1, as did:plc:mo7vq4jbioxwafy5zkcc3eux publishes them to HappyView's plugin
 * registry, by the CIDs of their manifest and module.
 */
const PLUGINS = {
	'happyview-lua': {
		manifest: 'bafkreiba4b5ytxsgam34rd6comf6ta6gv6joemzv2o6mr64iawykdjxxqi',
		module: 'bafkreidmwmmyu7ei3bi4j3svpekwqslc7x6uhsrcdy5bti3zbrvssnnwku'
	},
	'happyview-sql': {
		manifest: 'bafkreidre44qvi426myigls2c2hqwzkgf4ufbsr4hfvv4i36rdaopdsa2a',
		module: 'bafkreiemad6mqhaqocnvxnm53d2ptr57jomypbl7n2tctonwazhjmf2imi'
	},
	'happyview-db': {
		manifest: 'bafkreieorxqu54xzx5uwvxwx4cgyoaepltoosb5fjcpcku7ckpoe7ggoh4',
		module: 'bafkreiclfgauo627kgikb5uar7uye3ro2vuzhww7zbtpgc2m2kx62brpzy'
	},
	'happyview-http': {
		manifest: 'bafkreicgpxxchvhkr2naposqdnrn5dx7cn6vejyefe6telvrvjjxefovsq',
		module: 'bafkreigzybap3lobx7fec7v5oksjptd6qux57ufphljueiaxpiwhhq34nq'
	}
};

const REGISTRY_ARTIFACT = 'https://api.happyproto.at/xrpc/at.happyproto.registry.getArtifact';

/** Kept between runs, so only the first one downloads the plugins. */
const PLUGIN_CACHE = fileURLToPath(new URL('../../node_modules/.cache/happyview-plugins', import.meta.url));

const pluginManifest = z.object({ wasm_file: z.string() });

const ADMIN_DID = 'did:web:localhost';

const execFileAsync = promisify(execFile);

export type HappyView = { url: string; apiKey: string; stop(): Promise<void> };

/**
 * Runs a real HappyView in Docker for development and tests: host networking so it reaches the local PLC (through a
 * proxy that asks plc.directory for any other DID, so it can track network lexicons) and PDS, Postgres in a container
 * of its own, a Jetstream stand-in fed by the PDS firehose, and the PLUGINS the scripts need.
 * Resolves once it answers, with an admin API key. `stop()` removes both containers and their data.
 */
export async function startHappyView(options: { plcUrl: string; pdsUrl: string }): Promise<HappyView> {
	await checkDocker();

	for (const image of [IMAGE, POSTGRES_IMAGE]) {
		const hasImage = await docker('image', 'inspect', image).then(
			() => true,
			() => false
		);

		if (!hasImage) {
			console.log(`Pulling ${image} (first run only)`);
			await docker('pull', image);
		}
	}

	await cachePlugins();

	const name = `localnetwork-happyview-${randomBytes(4).toString('hex')}`;
	const database = `${name}-postgres`;
	const jetstream = await startJetstream(options);
	const plc = await startPlcProxy(options.plcUrl);
	const port = await freePort();
	const databasePort = await freePort();
	const url = `http://127.0.0.1:${port}`;
	const sessionSecret = randomBytes(32).toString('hex');
	const databasePassword = randomBytes(16).toString('hex');
	const remove = () => execFileSync('docker', ['rm', '--force', name, database], { stdio: 'ignore' });
	// The containers outlive this process, so an exit that skips stop() (a failed test setup) removes them too.
	process.once('exit', remove);

	const stop = async () => {
		process.off('exit', remove);
		await jetstream.close();
		await plc.close();
		remove();
	};

	const env = {
		DATABASE_URL: `postgres://happyview:${databasePassword}@127.0.0.1:${databasePort}/happyview`,
		PUBLIC_URL: url,
		HOST: '127.0.0.1',
		PORT: String(port),
		// The local PLC for the network's accounts, plc.directory for the network lexicons' authorities.
		PLC_URL: plc.url,
		// Only network-wide backfills ask the relay; targeted ones (with a DID) read the PDS.
		RELAY_URL: options.pdsUrl,
		JETSTREAM_URL: jetstream.url,
		// The stand-in serves only v1's /subscribe, and answers v2's path with a 400, which `auto` doesn't fall back on.
		JETSTREAM_PROTOCOL: 'v1',
		// Loaded before HappyView answers, so /health also means scripts can run.
		PLUGIN_DIR: '/plugins',
		// Nothing installs from the registry here, so HappyView never needs to read it.
		PLUGIN_REGISTRY_URL: 'off',
		SESSION_SECRET: sessionSecret,
		TOKEN_ENCRYPTION_KEY: randomBytes(32).toString('base64'),
		RUST_LOG: 'warn'
	};

	try {
		await docker(
			'run',
			'--detach',
			'--name',
			database,
			'--network',
			'host',
			// The data lives in memory and goes with the container.
			'--tmpfs',
			'/var/lib/postgresql/data',
			'--env',
			`PGPORT=${databasePort}`,
			'--env',
			'POSTGRES_USER=happyview',
			'--env',
			`POSTGRES_PASSWORD=${databasePassword}`,
			POSTGRES_IMAGE,
			// Host networking would otherwise put it on every interface.
			'-c',
			'listen_addresses=127.0.0.1'
		);

		await waitForPostgres(database, databasePort);

		await docker(
			'run',
			'--detach',
			'--name',
			name,
			'--network',
			'host',
			'--volume',
			`${PLUGIN_CACHE}:/plugins:ro`,
			...Object.entries(env).flatMap(([key, value]) => ['--env', `${key}=${value}`]),
			IMAGE
		);

		await waitForHealth(url, name);

		return { url, apiKey: await createAdminKey(url, sessionSecret), stop };
	} catch (error) {
		await stop().catch(() => undefined);
		throw error;
	}
}

/**
 * Fills the plugin cache, mounted as HappyView's PLUGIN_DIR: each plugin's manifest and the module it names, in a
 * directory of its own. A plugin loaded from there gets the capabilities its manifest declares.
 */
async function cachePlugins(): Promise<void> {
	for (const [id, cids] of Object.entries(PLUGINS)) {
		const dir = join(PLUGIN_CACHE, id);
		await mkdir(dir, { recursive: true });

		const manifest = pluginManifest.parse(
			JSON.parse((await cached(join(dir, 'manifest.json'), cids.manifest)).toString())
		);

		await cached(join(dir, manifest.wasm_file), cids.module);
	}
}

/** The file at `path`, first downloaded from the registry unless it's there already and matches `cid`. */
async function cached(path: string, cid: string): Promise<Buffer> {
	const expected = parseCid(cid);
	const existing = await readFile(path).catch(() => null);

	if (existing && (await isCidForBytes(expected, existing))) return existing;

	console.log(`Downloading ${cid} from HappyView's plugin registry (first run only)`);

	const response = await fetch(`${REGISTRY_ARTIFACT}?cid=${cid}`);

	if (!response.ok) throw new Error(`HappyView's plugin registry answered ${response.status} for ${cid}`);

	const bytes = Buffer.from(await response.arrayBuffer());

	if (!(await isCidForBytes(expected, bytes)))
		throw new Error(`HappyView's plugin registry sent other bytes for ${cid}`);

	await writeFile(path, bytes);

	return bytes;
}

const apiKeyOutput = z.object({ key: z.string() });

/**
 * With no SUPERADMIN_DID, whoever first uses HappyView's admin API with a session becomes its
 * super user. So this signs a session cookie the way its cookie library does, which
 * runs that bootstrap, and creates an API key with only the permissions the app documents, so
 * dev and tests catch it needing more.
 */
async function createAdminKey(url: string, sessionSecret: string): Promise<string> {
	// The cookie crate's `Key::derive_from`: HKDF-Expand with the secret as the PRK, signing key first.
	const signingKey = createHmac('sha256', sessionSecret)
		.update('COOKIE;SIGNED:HMAC-SHA256;PRIVATE:AEAD-AES-256-GCM\x01')
		.digest();

	// A signed cookie's value is the base64 HMAC-SHA256 of the value, then the value.
	const signature = createHmac('sha256', signingKey).update(ADMIN_DID).digest('base64');
	const headers = { cookie: `happyview_session=${signature}${ADMIN_DID}`, 'content-type': 'application/json' };

	const send = async (path: string, body?: { name: string; permissions: string[] }) => {
		const response = await fetch(`${url}${path}`, {
			method: body ? 'POST' : 'GET',
			headers,
			body: body && JSON.stringify(body)
		});

		if (!response.ok) throw new Error(`HappyView ${path} answered ${response.status}: ${await response.text()}`);

		return response.json();
	};

	// Any authenticated admin request makes the first caller the super user.
	await send('/admin/permissions');

	return apiKeyOutput.parse(
		await send('/admin/api-keys', { name: 'localnetwork', permissions: [...HAPPYVIEW_PERMISSIONS] })
	).key;
}

/**
 * Waits for Postgres to take connections over TCP. The image's first start runs its setup on a
 * server that only listens on a socket, then restarts it, so a TCP answer means setup is done.
 */
async function waitForPostgres(name: string, port: number): Promise<void> {
	const deadline = Date.now() + 30_000;

	while (Date.now() < deadline) {
		const ready = await docker('exec', name, 'pg_isready', '--host', '127.0.0.1', '--port', String(port)).then(
			() => true,
			() => false
		);

		if (ready) return;
		await sleep(100);
	}

	throw new Error(`Postgres did not take connections within 30s:\n${await docker('logs', '--tail', '20', name)}`);
}

async function waitForHealth(url: string, name: string): Promise<void> {
	const deadline = Date.now() + 30_000;

	while (Date.now() < deadline) {
		const healthy = await fetch(`${url}/health`).then(
			(response) => response.ok,
			() => false
		);

		if (healthy) return;
		await sleep(100);
	}

	throw new Error(`HappyView did not answer ${url}/health within 30s:\n${await docker('logs', '--tail', '20', name)}`);
}

async function freePort(): Promise<number> {
	const server = createServer().listen(0, '127.0.0.1');
	await once(server, 'listening');
	// SAFETY: a server listening on a TCP port reports an AddressInfo.
	const { port } = server.address() as AddressInfo;
	server.close();

	return port;
}

/**
 * Says what to do when Docker is missing or its daemon isn't running. `docker info` fails in both
 * cases, while other commands' errors for a stopped daemon vary between Docker versions.
 */
export async function checkDocker(): Promise<void> {
	await execFileAsync('docker', ['info']).catch((error) => {
		// SAFETY: execFile rejects with the spawn error code or the command's output.
		const isMissing = (error as NodeJS.ErrnoException).code === 'ENOENT';

		throw new Error(`HappyView runs in Docker: ${isMissing ? 'install' : 'start'} Docker (or OrbStack) first.`, {
			cause: error
		});
	});
}

/** Runs the docker CLI and returns everything it printed. */
export async function docker(...args: string[]): Promise<string> {
	try {
		const { stdout, stderr } = await execFileAsync('docker', args);

		return stdout + stderr;
	} catch (error) {
		// SAFETY: execFile rejects with the command's output.
		const { stderr } = error as { stderr?: string };

		throw new Error(`docker ${args[0]} failed: ${stderr?.trim()}`, { cause: error });
	}
}
