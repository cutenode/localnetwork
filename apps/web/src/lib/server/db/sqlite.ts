import { mkdirSync } from 'node:fs';
import { dirname } from 'node:path';
import { DatabaseSync, type SQLInputValue, type StatementSync } from 'node:sqlite';
import type { SqliteDatabase, SqliteStatement } from 'kysely';

/**
 * Adapts Node's built-in `node:sqlite` to the interface Kysely's
 * `SqliteDialect` expects (the better-sqlite3 shape), so the app has no
 * native dependencies.
 */
export class NodeSqliteDatabase implements SqliteDatabase {
	readonly raw: DatabaseSync;

	constructor(path: string) {
		if (path !== ':memory:') mkdirSync(dirname(path), { recursive: true });
		this.raw = new DatabaseSync(path);
		this.raw.exec('PRAGMA journal_mode = WAL');
		this.raw.exec('PRAGMA foreign_keys = ON');
		this.raw.exec('PRAGMA busy_timeout = 5000');
		this.raw.exec('PRAGMA synchronous = NORMAL');
	}

	prepare(sql: string): SqliteStatement {
		return new NodeSqliteStatement(this.raw.prepare(sql));
	}

	close(): void {
		if (this.raw.isOpen) this.raw.close();
	}
}

class NodeSqliteStatement implements SqliteStatement {
	readonly reader: boolean;

	constructor(private readonly statement: StatementSync) {
		this.reader = statement.columns().length > 0;
	}

	all(parameters: ReadonlyArray<unknown>): unknown[] {
		return this.statement.all(...toParams(parameters));
	}

	run(parameters: ReadonlyArray<unknown>) {
		const result = this.statement.run(...toParams(parameters));

		return { changes: result.changes, lastInsertRowid: result.lastInsertRowid };
	}

	iterate(parameters: ReadonlyArray<unknown>): IterableIterator<unknown> {
		return this.statement.iterate(...toParams(parameters));
	}
}

function toParams(parameters: ReadonlyArray<unknown>): SQLInputValue[] {
	return parameters.map((value) => {
		if (value === undefined) return null;

		if (value === true || value === false) return value ? 1 : 0;

		if (value instanceof Date) return value.toISOString();

		// SAFETY: besides the booleans and dates converted above, the app only binds strings, numbers, Uint8Arrays, and null
		// (our Database column types, plus the strings in its raw `sql` fragments), all of which are SQLInputValues.
		return value as SQLInputValue;
	});
}
