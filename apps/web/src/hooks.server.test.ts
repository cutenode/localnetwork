import { existsSync } from 'node:fs';
import { mkdtemp, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { init } from './hooks.server';

let dir: string;

beforeEach(async () => {
	dir = await mkdtemp(join(tmpdir(), 'hooks-server-test-'));
});

afterEach(async () => {
	vi.unstubAllEnvs();
	await rm(dir, { recursive: true, force: true });
});

it("doesn't start the app under test", async () => {
	// A configuration that gets as far as opening the database, then fails without leaving the machine.
	const databasePath = join(dir, 'app.db');
	vi.stubEnv('DATABASE_PATH', databasePath);
	vi.stubEnv('ROOT_IDENTIFIER', 'did:plc:hooks');
	vi.stubEnv('ROOT_PASSWORD', 'secret');
	vi.stubEnv('PLC_URL', 'http://127.0.0.1:9');
	vi.stubEnv('HAPPYVIEW_URL', 'http://127.0.0.1:9');
	vi.stubEnv('GEOCODER', 'none');

	await init();

	expect(existsSync(databasePath)).toBe(false);
});
