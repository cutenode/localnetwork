import { startPostgres } from './pglite';

/**
 * Makes the Postgres data directory the test emulator's Postgres starts from, once, before the server tests' workers
 * start. Without it, each worker on a fresh machine ran initdb inside its first test that used Postgres, all at once, and
 * on a loaded CI runner that alone ran past the test timeout.
 */
export default async function setup(): Promise<void> {
	const pg = await startPostgres();
	await pg.close();
}
