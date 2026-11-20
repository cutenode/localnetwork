import { readFile, rename, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { PGlite } from '@electric-sql/pglite';

/** A freshly made Postgres data directory, which every test process's Postgres starts from. */
const DATA_DIR = join(tmpdir(), 'localnetwork-pglite-data.tar');

/**
 * Postgres, from the data directory made before, since making one (initdb) takes about half a second on a quiet machine
 * and loading it a tenth of that. Without one, or with one this PGlite can't start from, it makes its own and saves it.
 * The server tests' global setup (pglite.setup.ts) calls this once before any test file runs, so the test files only
 * load it.
 */
export async function startPostgres(): Promise<PGlite> {
	try {
		return await PGlite.create({ loadDataDir: new Blob([await readFile(DATA_DIR)]) });
	} catch {
		const pg = await PGlite.create();
		const saving = `${DATA_DIR}.${process.pid}`;

		// Written aside, then moved into place, so a process starting at the same time never reads half of it.
		await writeFile(saving, (await pg.dumpDataDir('none')).stream());
		await rename(saving, DATA_DIR);

		return pg;
	}
}
