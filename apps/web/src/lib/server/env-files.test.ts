import { mkdtemp, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { loadEnvFiles } from './env-files';

const cwd = process.cwd();

let dir: string;

beforeEach(async () => {
	dir = await mkdtemp(join(tmpdir(), 'env-files-test-'));
	await writeFile(join(dir, '.env.local'), 'DATABASE_PATH=/env-local.db\n');
	await writeFile(join(dir, '.env'), 'DATABASE_PATH=/env.db\nPLC_URL=https://plc.env.example\n');
	process.chdir(dir);
	vi.stubEnv('DATABASE_PATH', undefined);
	vi.stubEnv('PLC_URL', undefined);
});

afterEach(async () => {
	process.chdir(cwd);
	vi.unstubAllEnvs();
	await rm(dir, { recursive: true, force: true });
});

it('reads .env.local, then .env, in development', () => {
	vi.stubEnv('NODE_ENV', 'development');
	loadEnvFiles();

	expect(process.env.DATABASE_PATH).toBe('/env-local.db');
	expect(process.env.PLC_URL).toBe('https://plc.env.example');
});

it('reads .env alone when there is no .env.local', async () => {
	vi.stubEnv('NODE_ENV', 'development');
	await rm(join(dir, '.env.local'));
	loadEnvFiles();

	expect(process.env.DATABASE_PATH).toBe('/env.db');
});

it('keeps variables the environment already sets', () => {
	vi.stubEnv('NODE_ENV', 'development');
	vi.stubEnv('DATABASE_PATH', '/real.db');
	loadEnvFiles();

	expect(process.env.DATABASE_PATH).toBe('/real.db');
});

it.each(['test', 'production'])('reads neither file under NODE_ENV=%s', (env) => {
	vi.stubEnv('NODE_ENV', env);
	loadEnvFiles();

	expect(process.env.DATABASE_PATH).toBeUndefined();
	expect(process.env.PLC_URL).toBeUndefined();
});
