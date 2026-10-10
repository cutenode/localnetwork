import { CamelCasePlugin, Kysely, SqliteDialect } from 'kysely';
import { migrateToLatest } from './migrations';
import type { Database } from './schema';
import { NodeSqliteDatabase } from './sqlite';

export type Db = Kysely<Database>;

export async function openDatabase(path: string): Promise<Db> {
	const db = new Kysely<Database>({
		dialect: new SqliteDialect({ database: new NodeSqliteDatabase(path) }),
		plugins: [new CamelCasePlugin()]
	});

	await migrateToLatest(db);

	return db;
}

export const nowIso = () => new Date().toISOString();

export function parseJson<T>(value: string | null | undefined, fallback: T): T {
	if (value === null || value === undefined || value === '') return fallback;

	try {
		// SAFETY: JSON columns are only written by this app, as JSON.stringify of the type their readers ask for (records are lexicon-validated before they're stored).
		return JSON.parse(value) as T;
	} catch {
		return fallback;
	}
}
