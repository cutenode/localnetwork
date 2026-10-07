import { execFile, execFileSync } from 'node:child_process';
import { createHmac, randomBytes } from 'node:crypto';
import { once } from 'node:events';
import { type AddressInfo, createServer } from 'node:net';
import { setTimeout as sleep } from 'node:timers/promises';
import { promisify } from 'node:util';
import { z } from 'zod';
import { HAPPYVIEW_PERMISSIONS } from '../../src/lib/server/happyview/client.ts';
import { startJetstream } from './jetstream.ts';

/** Also pulled by the integration and e2e jobs in .github/workflows/ci.yml. */
const IMAGE = 'ghcr.io/gamesgamesgamesgamesgames/happyview:2.16.0';

const ADMIN_DID = 'did:web:localhost';

const execFileAsync = promisify(execFile);

export type HappyView = { url: string; apiKey: string; stop(): Promise<void> };

/**
 * Runs a real HappyView in Docker for development and tests: host networking so it reaches the
 * local PLC and PDS, SQLite inside the container, and a Jetstream stand-in fed by the PDS firehose.
 * Resolves once it answers, with an admin API key. `stop()` removes the container and its data.
 */
export async function startHappyView(options: { plcUrl: string; pdsUrl: string }): Promise<HappyView> {
	const hasImage = await docker('image', 'inspect', IMAGE).then(
		() => true,
		() => false
	);

	if (!hasImage) {
		console.log(`Pulling ${IMAGE} (first run only)`);
		await docker('pull', IMAGE);
	}

	const name = `localnetwork-happyview-${randomBytes(4).toString('hex')}`;
	const jetstream = await startJetstream(options);
	const port = await freePort();
	const url = `http://127.0.0.1:${port}`;
	const sessionSecret = randomBytes(32).toString('hex');
	const remove = () => execFileSync('docker', ['rm', '--force', name], { stdio: 'ignore' });
	// The container outlives this process, so an exit that skips stop() (a failed test setup) removes it too.
	process.once('exit', remove);

	const stop = async () => {
		process.off('exit', remove);
		await jetstream.close();
		remove();
	};

	const env = {
		DATABASE_URL: 'sqlite:///tmp/happyview.db?mode=rwc',
		PUBLIC_URL: url,
		HOST: '127.0.0.1',
		PORT: String(port),
		PLC_URL: options.plcUrl,
		// Only network-wide backfills ask the relay; targeted ones (with a DID) read the PDS.
		RELAY_URL: options.pdsUrl,
		JETSTREAM_URL: jetstream.url,
		SESSION_SECRET: sessionSecret,
		TOKEN_ENCRYPTION_KEY: randomBytes(32).toString('base64'),
		RUST_LOG: 'warn'
	};

	try {
		await docker(
			'run',
			'--detach',
			'--name',
			name,
			'--network',
			'host',
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

const apiKeyOutput = z.object({ key: z.string() });

/**
 * HappyView has no admin bootstrap from env: whoever first uses the admin API with a session
 * becomes its super user. So this signs a session cookie the way its cookie library does, which
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

/** Runs the docker CLI and returns everything it printed. */
async function docker(...args: string[]): Promise<string> {
	try {
		const { stdout, stderr } = await execFileAsync('docker', args);

		return stdout + stderr;
	} catch (error) {
		// SAFETY: execFile rejects with the spawn error code or the command's output.
		const { code, stderr } = error as NodeJS.ErrnoException & { stderr?: string };

		if (code === 'ENOENT') {
			throw new Error('HappyView runs in Docker: install Docker (or OrbStack) first.', { cause: error });
		}

		throw new Error(`docker ${args[0]} failed: ${stderr?.trim()}`, { cause: error });
	}
}
