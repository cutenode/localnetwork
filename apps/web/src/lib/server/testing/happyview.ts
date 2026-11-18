import { readFile, rename, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { PGlite } from '@electric-sql/pglite';
import { parse } from 'libpg-query';
import { type LexiconDoc, Lexicons, type LexXrpcParameters, ValidationError } from '@atproto/lexicon';
import type { JsonObject, JsonValue } from '@atproto/lex-json';
import { LUA_REGISTRYINDEX, LuaEventMasks, LuaFactory, LuaReturn, type LuaState, LuaType, type LuaWasm } from 'wasmoon';
import { REFERENCED_DOCS } from '../atproto/lexicons';
import type { JsonRecord } from '../atproto/repo-client';
import { z } from 'zod';
import { type HappyViewBundle, happyViewBundle } from '../happyview/bundle';
import { REMOVED_GLOBALS, removedGlobals } from './removed-globals';

/**
 * HappyView 3 stops a script after this many Lua instructions (its default `SCRIPT_INSTRUCTION_LIMIT`), counted apart
 * while the script loads (also at upload) and in handle(). A pcall can't save a run past it: the run still fails.
 */
const INSTRUCTION_LIMIT = 1_000_000;

/** How often the count hook here fires, which is also how precisely `instructions` counts. */
const HOOK_INTERVAL = 1_000;

/** Registry name of the metatable that marks a table as a JSON array, like mlua's array metatable. */
const ARRAY = 'array';

/**
 * What HappyView's sandbox leaves of Lua's standard library, and its guard on the globals v3 removed: reading one
 * raises, naming it, while assigning one still works.
 */
const SANDBOX = `local system = os
os = { time = system.time, date = system.date, difftime = system.difftime, clock = system.clock }
io, debug, package, require, dofile, loadfile, load, collectgarbage = nil
local removed = { ${REMOVED_GLOBALS.map((name) => `${name} = true`).join(', ')} }
setmetatable(_G, { __index = function(_, name)
	if removed[name] then
		error("the '" .. name .. "' global was removed in v3; run the script codemod (Migrate on the script's page under"
			.. " Integrations → Scripts, or happyview-codemod) -- see the Migrating scripts guide", 2)
	end
end })`;

/**
 * The require HappyView checks a script with on upload, which has no instance behind it: every module is a table
 * whose every field is a function returning another such table, so any chain of calls at the top level loads.
 */
const STUB_REQUIRE = `local function stub()
	return setmetatable({}, { __index = function() return function() return stub() end end })
end
require = function() return stub() end`;

/**
 * HappyView's records as the hosted instance's record store keeps them, in Postgres: a table keyed by account and
 * collection ids, behind the view scripts read, whose `uri` is computed from did, collection and rkey when it's read,
 * whose `record` is text, and whose timestamps read as text in UTC.
 */
const SCHEMA = `CREATE TABLE happyview_dids (id serial PRIMARY KEY, did text NOT NULL UNIQUE);
CREATE TABLE happyview_collections (id serial PRIMARY KEY, nsid text NOT NULL UNIQUE);
CREATE TABLE happyview_records_v2 (
	id bigserial PRIMARY KEY,
	did_id integer NOT NULL REFERENCES happyview_dids (id),
	collection_id integer NOT NULL REFERENCES happyview_collections (id),
	rkey text NOT NULL,
	cid text NOT NULL,
	body text NOT NULL,
	indexed_at timestamptz,
	created_at timestamptz NOT NULL DEFAULT now(),
	UNIQUE (did_id, collection_id, rkey)
);
CREATE VIEW happyview_records AS
	SELECT 'at://' || d.did || '/' || c.nsid || '/' || v.rkey AS uri, d.did, c.nsid AS collection, v.rkey,
		v.body AS record, v.cid,
		to_char(v.indexed_at AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.US') || '+00:00' AS indexed_at,
		to_char(v.created_at AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.US') || '+00:00' AS created_at
	FROM happyview_records_v2 v
	JOIN happyview_dids d ON d.id = v.did_id
	JOIN happyview_collections c ON c.id = v.collection_id`;

/**
 * The functions HappyView 3 lets plugin raw SQL call on Postgres, from `ALWAYS_FORBIDDEN_FUNCTIONS` and
 * `PLUGIN_SQL_FUNCTIONS` in its src/raw_sql_guard.rs at v3.0.0-beta.14. It refuses the forbidden ones everywhere, and
 * anything not allowed (or of a schema other than public or pg_catalog) unless the SQL runs on a least-privileged
 * login pool. The dev container checks that, and the hosted store's route didn't, so the scripts keep to it.
 */
const PLUGIN_SQL = {
	forbidden: new Set([
		'set_config',
		'query_to_xml',
		'query_to_xmlschema',
		'query_to_xml_and_xmlschema',
		'cursor_to_xml',
		'cursor_to_xmlschema',
		'ts_stat',
		'ts_rewrite',
		'dblink',
		'dblink_exec',
		'dblink_connect',
		'dblink_connect_u',
		'pg_advisory_lock',
		'pg_advisory_lock_shared',
		'pg_advisory_unlock',
		'pg_advisory_unlock_shared',
		'pg_advisory_unlock_all',
		'pg_try_advisory_lock',
		'pg_try_advisory_lock_shared',
		'lo_import',
		'lo_export',
		'lo_create',
		'lo_creat',
		'lo_unlink',
		'lo_from_bytea',
		'lo_put',
		'lo_get',
		'lo_open',
		'lowrite',
		'loread',
		'pg_read_file',
		'pg_read_binary_file',
		'pg_ls_dir',
		'pg_stat_file'
	]),
	allowed: new Set([
		'count',
		'sum',
		'avg',
		'min',
		'max',
		'coalesce',
		'nullif',
		'greatest',
		'least',
		'lower',
		'upper',
		'length',
		'char_length',
		'trim',
		'btrim',
		'ltrim',
		'rtrim',
		'substr',
		'substring',
		'left',
		'right',
		'replace',
		'concat',
		'concat_ws',
		'split_part',
		'position',
		'strpos',
		'starts_with',
		'format',
		'md5',
		'encode',
		'regexp_replace',
		'abs',
		'round',
		'ceil',
		'ceiling',
		'floor',
		'mod',
		'power',
		'sign',
		'now',
		'statement_timestamp',
		'clock_timestamp',
		'date_trunc',
		'to_timestamp',
		'to_char',
		'to_date',
		'extract',
		'date_part',
		'age',
		'make_timestamptz',
		'row_number',
		'rank',
		'dense_rank',
		'json_extract_path_text',
		'jsonb_extract_path_text',
		'json_array_length',
		'jsonb_array_length',
		'json_build_object',
		'jsonb_build_object',
		'json_build_array',
		'jsonb_build_array',
		'to_json',
		'to_jsonb',
		'jsonb_array_elements',
		'jsonb_array_elements_text',
		'jsonb_object_keys',
		'array_agg',
		'string_agg',
		'jsonb_agg',
		'json_agg',
		'unnest'
	])
};

/** A function call in Postgres's parse tree, as libpg-query writes it: its name, schema first when it has one. */
const FUNC_CALL = /"FuncCall":\{"funcname":(\[(?:\{"String":\{"sval":"(?:[^"\\]|\\.)*"\}\},?)*\])/g;

const FUNC_NAME = z.array(z.object({ String: z.object({ sval: z.string() }) }));

/**
 * Refuses `sql` for the functions it calls, as HappyView's check_postgres_plugin_sql does, reading them from
 * Postgres's own parser (libpg_query, which HappyView uses too), so a cast, an operator or coalesce isn't a call.
 */
async function checkFunctions(sql: string): Promise<void> {
	const tree = JSON.stringify(
		await parse(sql).catch((error: Error) => {
			throw new Error(`could not parse SQL: ${error.message}`, { cause: error });
		})
	);

	for (const [, names] of tree.matchAll(FUNC_CALL)) {
		const parts = FUNC_NAME.parse(JSON.parse(names)).map(({ String: { sval } }) => sval);
		const func = parts.join('.');
		const name = parts[parts.length - 1].toLowerCase();
		const schema = parts.length > 1 ? parts[0].toLowerCase() : 'public';

		if (PLUGIN_SQL.forbidden.has(name)) throw new Error(`raw SQL cannot call the function '${func}'`);

		if (schema !== 'public' && schema !== 'pg_catalog')
			throw new Error(`raw SQL cannot call '${func}' (schema '${parts[0]}' is not allowed)`);

		if (!PLUGIN_SQL.allowed.has(name))
			throw new Error(`raw SQL cannot call the function '${func}'; it is not on the plugin SQL allowlist`);
	}
}

/** The record views, which the store answers alone: a query reading one can't read any other table. */
const RECORD_VIEWS = new Set(['happyview_records', 'happyview_record_refs']);

/** Postgres type OIDs by how sqlx's `try_get` chain reads them: text, then integer, then real, then boolean. */
const TEXT_TYPES = new Set([18, 19, 25, 705, 1042, 1043]);

const INTEGER_TYPES = new Set([20, 21, 23]);

const REAL_TYPES = new Set([700, 701]);

const BOOLEAN = 16;

/** Postgres's text for each of those, which `pushRows` reads itself, rather than PGlite's JavaScript values. */
const AS_TEXT = Object.fromEntries(
	[...TEXT_TYPES, ...INTEGER_TYPES, ...REAL_TYPES, BOOLEAN].map((type) => [type, (text: string) => text])
);

/** A query's rows as Postgres answers them, each a value per column, with the columns' names and types. */
type Rows = { fields: { name: string; dataTypeID: number }[]; rows: (string | null)[][] };

/** One FakeHappyView's tables in the Postgres every instance in the process shares. */
type Database = { query(statement: string, params: unknown[]): Promise<Rows> };

/**
 * The internal tables HappyView 3 lets raw SQL name (`ALLOWED_INTERNAL_TABLES` in its src/raw_sql_guard.rs); it
 * refuses SQL naming any other `happyview_*` table, such as its backfill jobs or the records table behind the view, so
 * a script can't come to depend on one.
 */
const READABLE_TABLES = new Set([
	'happyview_records',
	'happyview_record_refs',
	'happyview_records_by_collection',
	'happyview_labels',
	'happyview_lexicons',
	'happyview_jobs'
]);

type ScriptErrorType = 'syntax' | 'runtime' | 'timeout' | 'missing_handle';

/** A failed run, as HappyView's `AppError::ScriptError` reports it. */
class ScriptError extends Error {
	constructor(
		readonly errorType: ScriptErrorType,
		message: string
	) {
		super(message);
	}
}

/** What handle() returned: its Lua type, and the JSON HappyView makes of it. */
type ScriptResult = { type: LuaType; value: JsonValue };

/** handle()'s two arguments. */
type Call = { input: JsonValue; ctx: JsonValue };

/**
 * A run's last library call, which handle() waits on: the call, which answers with how to push its result, then that
 * or why it failed.
 */
type Pending = { query: Promise<Answer>; outcome?: { answer: Answer } | { error: Error } };

/** What answers a script's HTTP GET of `url`, in place of the network: a response, or nothing when it can't connect. */
export type Web = (url: string) => { status: number; body: string } | undefined;

/** Pushes a library call's result onto the coroutine's stack. */
type Answer = (L: LuaState) => void;

/** A `happyview.db` chain's lazy steps, as its Lua side collects them (an empty `where` list arrives as an object). */
const RECORDS = z.object({
	collection: z.string(),
	where: z
		.array(z.tuple([z.string(), z.string(), z.union([z.string(), z.number(), z.boolean()])]))
		.or(z.object({}).transform(() => [])),
	sort: z.tuple([z.string(), z.enum(['asc', 'desc']).optional()]).optional(),
	limit: z.number().int().positive().optional(),
	cursor: z.string().optional(),
	did: z.string().optional()
});

/**
 * `happyview.db`'s surface, around the library call: `records(collection)` answers a chain whose lazy steps (`where`,
 * `sort`, `limit`, `cursor`, `did`) each return it, skipped when given nil, and whose immediate calls (`run`, `count`,
 * `first`) make the call.
 */
const DB_MODULE = `local library = ...
local db = {}
function db.records(collection)
	local spec = { collection = collection, where = {} }
	local chain = {}
	function chain:where(field, op, value)
		if field ~= nil and op ~= nil and value ~= nil then spec.where[#spec.where + 1] = { field, op, value } end
		return self
	end
	function chain:sort(field, direction)
		if field ~= nil then spec.sort = { field, direction } end
		return self
	end
	function chain:limit(limit)
		spec.limit = limit
		return self
	end
	function chain:cursor(cursor)
		spec.cursor = cursor
		return self
	end
	function chain:did(did)
		spec.did = did
		return self
	end
	function chain:run() return library('run', spec) end
	function chain:count() return library('count', spec) end
	function chain:first() return library('first', spec) end
	return chain
end
return db`;

/** The comparisons `where` takes, as SQL. */
const OPERATORS = new Map([
	['=', '='],
	['!=', '<>'],
	['>', '>'],
	['<', '<'],
	['>=', '>='],
	['<=', '<=']
]);

/** The text matches `where` takes, which compare a field's text whatever it holds. */
const MATCHES = new Map([
	['like', 'LIKE'],
	['not like', 'NOT LIKE'],
	['ilike', 'ILIKE']
]);

// What's left of a JSON value once the other kinds are ruled out.
const isText = <T>(value: T): value is Extract<T, string> => value === String(value);

const isNumber = <T>(value: T): value is Extract<T, number> => value === Number(value);

/** happyview.db's envelopes for rows of the records view, each with its body as stored. */
function envelopes({ fields, rows }: Rows): JsonObject[] {
	return rows.map((row) => {
		const envelope: JsonObject = {};

		fields.forEach(({ name }, column) => {
			const value = row[column];

			if (value !== null) envelope[name] = name === 'record' ? z.json().parse(JSON.parse(value)) : value;
		});

		return envelope;
	});
}

/**
 * Lua 5.4 (wasmoon) set up the way HappyView 3's runner sets up mlua: a fresh state per run, its sandbox and guard on
 * removed globals, `require` with the modules the scripts use (`internal.json`, `internal.logging` and
 * `happyview.sql`'s `raw`, over Postgres), and the instruction limit. handle() runs in a coroutine, as it does in
 * HappyView, so `sql.raw` can yield while Postgres answers.
 */
class Engine {
	/** Lua instructions the last run used, counted in steps of `HOOK_INTERVAL`. */
	instructions = 0;
	/** The run in progress. Runs wait their turn, so there is never more than one. */
	private run?: { db: Database; logs: string[]; web: Web; ticks: number; tripped: boolean; pending?: Pending };
	/** The last run queued, which the next one starts after. */
	private queue: Promise<unknown> = Promise.resolve();
	private readonly hook: number;
	private readonly require: number;
	/** Where a library call picks up once it has answered. */
	private readonly answered: number;
	/** `happyview.db`'s one library call, which its chains make. */
	private readonly records: number;
	/** The functions of each module `require` gives, by module name. */
	private readonly modules: Record<string, Record<string, number>>;
	/** Script bodies that loaded the way HappyView checks one on upload. */
	private readonly uploadable = new Set<string>();

	constructor(private readonly lua: LuaWasm) {
		this.hook = lua.module.addFunction((L: LuaState) => {
			const run = this.current();
			run.ticks += 1;

			if (run.ticks * HOOK_INTERVAL < INSTRUCTION_LIMIT) return;
			run.tripped = true;
			lua.lua_pushstring(L, 'script exceeded execution limit');
			lua.lua_error(L);
		}, 'vii');

		const log = this.cFunction('log', (L) => {
			this.current().logs.push(lua.luaL_checklstring(L, 1, null));

			if (lua.lua_type(L, 2) > LuaType.Nil) lua.luaL_checktype(L, 2, LuaType.Table);

			return 0;
		});

		this.modules = {
			'internal.json': {
				encode: this.cFunction('json.encode', (L) => {
					lua.lua_pushstring(L, JSON.stringify(this.toJson(L, 1)));

					return 1;
				}),
				decode: this.cFunction('json.decode', (L) => {
					this.push(L, JSON.parse(lua.luaL_checklstring(L, 1, null)));

					return 1;
				}),
				to_array: this.cFunction('json.to_array', (L) => {
					lua.luaL_checktype(L, 1, LuaType.Table);
					lua.lua_createtable(L, 0, 0);

					for (let i = 1n; lua.lua_rawgeti(L, 1, i) !== LuaType.Nil; i++) lua.lua_rawseti(L, -2, i);
					lua.lua_settop(L, -2);
					lua.luaL_setmetatable(L, ARRAY);

					return 1;
				})
			},
			'internal.logging': { debug: log, info: log, warn: log, error: log },
			'happyview.sql': { raw: this.cFunction('happyview-sql.raw', (L) => this.raw(L)) },
			'happyview.http': { get: this.cFunction('happyview-http.get', (L) => this.get(L)) }
		};

		this.records = this.cFunction('happyview-db.records', (L) => this.read(L));

		this.answered = this.cFunction(
			'library call',
			(L) => {
				const run = this.current();
				const outcome = run.pending?.outcome;
				run.pending = undefined;

				if (!outcome) throw new Error('resumed before the query answered');

				if ('error' in outcome) throw outcome.error;
				outcome.answer(L);

				return 1;
			},
			'iiii'
		);

		this.require = this.cFunction('require', (L) => {
			const name = lua.luaL_checklstring(L, 1, null);
			const key = `happyview.require.${name}`;

			// Each module is built once per run.
			if (lua.lua_getfield(L, LUA_REGISTRYINDEX, key) === LuaType.Table) return 1;

			if (name === 'happyview.db') {
				lua.luaL_loadstring(L, DB_MODULE);
				lua.lua_pushcclosure(L, this.records, 0);
				lua.lua_callk(L, 1, 1, 0, null);
				lua.lua_pushvalue(L, -1);
				lua.lua_setfield(L, LUA_REGISTRYINDEX, key);

				return 1;
			}

			const functions = this.modules[name];

			if (!functions) throw new Error(`module '${name}' not found -- is the '${name}' library plugin installed?`);
			lua.lua_createtable(L, 0, 0);

			for (const [field, pointer] of Object.entries(functions)) {
				lua.lua_pushcclosure(L, pointer, 0);
				lua.lua_setfield(L, -2, field);
			}

			lua.lua_pushvalue(L, -1);
			lua.lua_setfield(L, LUA_REGISTRYINDEX, key);

			return 1;
		});
	}

	/**
	 * Loads `body` into a fresh state and calls its handle(input, ctx) in a coroutine, answering each `sql.raw` it
	 * waits on. Loading and handle() each get the full instruction budget.
	 */
	execute(
		db: Database,
		logs: string[],
		web: Web,
		name: string,
		body: string,
		{ input, ctx }: Call
	): Promise<ScriptResult> {
		return this.session(db, logs, web, async (L) => {
			const { lua } = this;
			lua.lua_pushcclosure(L, this.require, 0);
			lua.lua_setglobal(L, 'require');
			this.load(L, name, body);
			const thread = lua.lua_newthread(L);
			lua.lua_pushvalue(L, -2);
			lua.lua_xmove(L, thread, 1);
			this.push(thread, input);
			this.push(thread, ctx);
			// Where lua_resume writes how many results the coroutine left, a userdata so the state frees it.
			const count = lua.lua_newuserdatauv(L, 4, 0);

			let status = lua.lua_resume(thread, L, 2, count);

			while (status === LuaReturn.Yield) {
				const pending = this.current().pending;

				if (!pending) throw new ScriptError('runtime', 'attempt to yield across a C-call boundary');
				pending.outcome = await pending.query.then(
					(answer) => ({ answer }),
					(error: Error) => ({ error })
				);
				status = lua.lua_resume(thread, L, 0, count);
			}

			const failed = status === LuaReturn.Ok ? undefined : lua.luaL_tolstring(thread, -1, null);

			if (failed?.includes('execution limit') || this.current().tripped)
				throw new ScriptError('timeout', 'script exceeded execution time limit');

			if (failed !== undefined) throw new ScriptError('runtime', failed);
			const results = lua.module.getValue(count, 'i32');
			const at = lua.lua_gettop(thread) - results + 1;

			try {
				return {
					type: results ? lua.lua_type(thread, at) : LuaType.Nil,
					value: results ? this.toJson(thread, at) : null
				};
			} catch (error) {
				throw new ScriptError('runtime', error instanceof Error ? error.message : String(error));
			}
		});
	}

	/**
	 * Checks `body` the way HappyView does on upload: it has to load, with a stand-in `require` and nothing to read,
	 * and define handle(). Throws what HappyView answers when it doesn't.
	 */
	async upload(db: Database, name: string, body: string): Promise<void> {
		if (this.uploadable.has(body)) return;

		await this.session(
			db,
			[],
			() => undefined,
			(L) => {
				this.lua.luaL_loadstring(L, STUB_REQUIRE);
				this.lua.lua_callk(L, 0, 0, 0, null);

				try {
					this.load(L, name, body);
				} catch (error) {
					const message = error instanceof Error ? error.message : String(error);
					throw new Error(`${name}: script compilation failed: ${message}`, { cause: error });
				}

				return Promise.resolve();
			}
		);
		this.uploadable.add(body);
	}

	/** Runs `use` on a fresh, sandboxed state as the run in progress, after the runs before it, then closes it. */
	private session<T>(db: Database, logs: string[], web: Web, use: (L: LuaState) => Promise<T>): Promise<T> {
		const turn = this.queue.then(async () => {
			const { lua } = this;
			const L = lua.luaL_newstate();
			const run = { db, logs, web, ticks: 0, tripped: false };
			this.run = run;

			try {
				lua.luaL_openlibs(L);
				lua.luaL_loadstring(L, SANDBOX);
				lua.lua_callk(L, 0, 0, 0, null);
				lua.luaL_newmetatable(L, ARRAY);
				lua.lua_settop(L, 0);

				return await use(L);
			} finally {
				this.instructions = run.ticks * HOOK_INTERVAL;
				this.run = undefined;
				lua.lua_close(L);
			}
		});

		this.queue = turn.catch(() => undefined);

		return turn;
	}

	/** Loads and runs `body`'s top level under the instruction limit, then pushes its handle() for a fresh budget. */
	private load(L: LuaState, name: string, body: string): void {
		const { lua } = this;
		const run = this.current();
		lua.lua_sethook(L, this.hook, LuaEventMasks.Count, HOOK_INTERVAL);

		const loaded =
			lua.luaL_loadbufferx(L, body, lua.module.lengthBytesUTF8(body), `=${name}`, null) === LuaReturn.Ok
				? this.call(L, 0, 0)
				: lua.lua_tolstring(L, -1, null);

		if (loaded !== undefined) throw new ScriptError(run.tripped ? 'timeout' : 'syntax', loaded);
		run.ticks = 0;
		lua.lua_sethook(L, this.hook, LuaEventMasks.Count, HOOK_INTERVAL);

		if (lua.lua_getglobal(L, 'handle') !== LuaType.Function) {
			throw new ScriptError('missing_handle', 'script does not define a handle() function');
		}
	}

	private current() {
		if (!this.run) throw new Error('No script is running');

		return this.run;
	}

	/** Calls the function under `args` arguments on the stack, keeping `results` of its results. Returns the error, if any. */
	private call(L: LuaState, args: number, results: number): string | undefined {
		// 0 is LUA_OK; wasmoon types the status as a plain number.
		if (this.lua.lua_pcallk(L, args, results, 0, 0, null) === 0) return undefined;

		return this.lua.luaL_tolstring(L, -1, null);
	}

	/**
	 * A Lua C function (or, with signature `iiii`, a continuation) that runs `body`, which returns how many results it
	 * pushed. JS errors become Lua errors.
	 */
	private cFunction(name: string, body: (L: LuaState) => number, signature = 'ii'): number {
		return this.lua.module.addFunction((L: LuaState) => {
			try {
				return body(L);
			} catch (error) {
				// Emscripten unwinds a Lua error (a longjmp) by throwing Infinity, which has to pass through.
				if (error === Infinity) throw error;
				this.lua.lua_pushstring(L, `${name}: ${error instanceof Error ? error.message : String(error)}`);
			}

			return this.lua.lua_error(L);
		}, signature);
	}

	/**
	 * `sql.raw(sql, params)`: refuses what the record store refuses, binds like HappyView's host, then yields until
	 * Postgres answers.
	 */
	private raw(L: LuaState): number {
		const { lua } = this;
		const sql = lua.luaL_checklstring(L, 1, null);
		const values: unknown[] = [];
		const tables = new Set(sql.match(/\bhappyview_\w+/gi)?.map((table) => table.toLowerCase()));
		const internal = [...tables].find((table) => !READABLE_TABLES.has(table));

		if (internal) throw new Error(`raw SQL cannot reference the protected internal HappyView table '${internal}'`);

		const readsRecords = [...tables].some((table) => RECORD_VIEWS.has(table));

		if (readsRecords && [...tables].some((table) => !RECORD_VIEWS.has(table)))
			throw new Error('FORBIDDEN: a query that reads the record views reads no other table');

		// HappyView parses raw SQL with sqlparser's generic dialect, which only takes MATERIALIZED in Postgres mode, and
		// refuses a query it can't parse as read-only as a write to the claimed records it names.
		if (readsRecords && /\bMATERIALIZED\b/i.test(sql))
			throw new Error(
				'FORBIDDEN: writes to records of a collection stored outside this instance must go through save_local'
			);

		if (lua.lua_type(L, 2) === LuaType.Table) {
			// An empty table is an object to mlua unless json.to_array marked it.
			if (Number(lua.lua_rawlen(L, 2)) === 0 && !this.isArray(L, 2)) throw new Error('params must be an array');

			// A Lua sequence ends at its first nil, so a nil param ends the list.
			for (let i = 1n; lua.lua_rawgeti(L, 2, i) !== LuaType.Nil; i++) {
				values.push(this.parameter(L));
				lua.lua_settop(L, -2);
			}
		}

		const { db } = this.current();

		const answer = checkFunctions(sql)
			.then(() => db.query(sql, values))
			.then(
				(rows): Answer =>
					(state) =>
						this.pushRows(state, rows)
			);

		this.current().pending = { query: answer };

		return lua.lua_yieldk(L, 0, 0, this.answered);
	}

	/**
	 * `happyview.db`'s call for a chain, as v3's `records_query` and `records_count` run it (src/record_store/query.rs):
	 * `run` answers a page of envelopes (`{ uri, did, collection, rkey, cid, indexed_at, record }`, the body as stored),
	 * at most `limit` (20 by default, 100 at most), with a cursor whenever the page is full; `count` how many match;
	 * `first` the first envelope or nil. A `where` compares the field's JSON with its value's JSON, so a string matches
	 * only a string; `like`, `not like` and `ilike` compare the field's text. Records come newest first, or by `sort`
	 * (descending unless asked otherwise).
	 */
	private read(L: LuaState): number {
		const { lua } = this;
		const call = lua.luaL_checklstring(L, 1, null);
		const spec = RECORDS.parse(this.toJson(L, 2));
		const values: (string | number | string[])[] = [spec.collection];
		const bind = (value: (typeof values)[number]) => `$${values.push(value)}`;
		const where = ['c.nsid = $1'];

		if (spec.did !== undefined) where.push(`d.did = ${bind(spec.did)}`);

		const field = (path: string) => `v.body::jsonb #> ${bind(path.replaceAll(/\[(\d+)\]/g, '.$1').split('.'))}::text[]`;

		for (const [path, op, value] of spec.where) {
			const match = MATCHES.get(op.toLowerCase());
			const operator = OPERATORS.get(op);

			if (match) where.push(`(${field(path)}) #>> '{}' ${match} ${bind(String(value))}`);
			else if (operator) where.push(`${field(path)} ${operator} CAST(${bind(JSON.stringify(value))} AS jsonb)`);
			else throw new Error(`BAD_INPUT: invalid filter op '${op}'`);
		}

		const from = ` FROM happyview_records_v2 v JOIN happyview_dids d ON d.id = v.did_id
			JOIN happyview_collections c ON c.id = v.collection_id WHERE ${where.join(' AND ')}`;

		const [sortField, direction = 'desc'] = spec.sort ?? [];
		const nulls = direction === 'asc' ? 'NULLS FIRST' : 'NULLS LAST';
		const sort = sortField === undefined ? '' : `${field(sortField)} ${direction.toUpperCase()} ${nulls}, `;
		const order = ` ORDER BY ${sort}v.created_at DESC, v.id DESC`;

		const envelope = `SELECT 'at://' || d.did || '/' || c.nsid || '/' || v.rkey AS uri, d.did, c.nsid AS collection,
			v.rkey, v.cid, to_char(v.indexed_at AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.US') || '+00:00' AS indexed_at,
			v.body AS record`;

		const { db } = this.current();
		let answer: Promise<Answer>;

		if (call === 'count') {
			answer = db.query(`SELECT count(*)${from}`, values).then(
				(rows): Answer =>
					(state) =>
						lua.lua_pushinteger(state, BigInt(rows.rows[0][0] ?? 0))
			);
		} else if (call === 'first') {
			answer = db.query(`${envelope}${from}${order} LIMIT 1`, values).then((rows): Answer => {
				const [first] = envelopes(rows);

				return (state) => (first ? this.push(state, first) : lua.lua_pushnil(state));
			});
		} else if (call === 'run') {
			const offset = spec.cursor === undefined ? 0 : Number(Buffer.from(spec.cursor, 'base64').toString());

			if (!Number.isSafeInteger(offset) || offset < 0)
				throw new Error('BAD_INPUT: invalid cursor: request the first page again');
			const limit = Math.max(1, Math.min(100, spec.limit ?? 20));

			answer = db
				.query(`${envelope}${from}${order} LIMIT ${bind(limit)} OFFSET ${bind(offset)}`, values)
				.then((rows): Answer => {
					const records = envelopes(rows);
					const cursor = Buffer.from(String(offset + limit)).toString('base64');
					// HappyView sends a cursor whenever the page is full, so the last page can come back empty.
					const page = records.length === limit ? { records, cursor } : { records };

					return (state) => this.push(state, page);
				});
		} else throw new Error(`no such call '${call}'`);

		this.current().pending = { query: answer };

		return lua.lua_yieldk(L, 0, 0, this.answered);
	}

	/**
	 * `http.get(url)`: the response `web` gives, as `{ status, body, headers }`, after a yield like any library call.
	 * A URL it can't connect to raises, as the library does.
	 */
	private get(L: LuaState): number {
		const url = this.lua.luaL_checklstring(L, 1, null);
		const response = this.current().web(url);

		const answer = response
			? Promise.resolve<Answer>((state) => this.push(state, { ...response, headers: {} }))
			: Promise.reject(new Error(`could not connect to ${url}`));

		this.current().pending = { query: answer };

		return this.lua.lua_yieldk(L, 0, 0, this.answered);
	}

	/** Rows as `sql.raw` gives them: a table per row, each column converted like HappyView's sqlx `try_get` chain. */
	private pushRows(L: LuaState, { fields, rows }: Rows): void {
		const { lua } = this;
		lua.lua_createtable(L, rows.length, 0);

		rows.forEach((row, i) => {
			lua.lua_createtable(L, 0, 0);

			fields.forEach(({ name, dataTypeID: type }, column) => {
				const value = row[column];

				// A NULL, or a type the chain can't read (numeric, json, a timestamp), fails every try and comes out nil.
				if (value === null) return;

				if (TEXT_TYPES.has(type)) lua.lua_pushstring(L, value);
				else if (INTEGER_TYPES.has(type)) lua.lua_pushinteger(L, BigInt(value));
				else if (REAL_TYPES.has(type)) lua.lua_pushnumber(L, Number(value));
				else if (type === BOOLEAN) lua.lua_pushboolean(L, value === 't' ? 1 : 0);
				else return;
				lua.lua_setfield(L, -2, name);
			});

			lua.lua_rawseti(L, -2, BigInt(i + 1));
		});
		lua.luaL_setmetatable(L, ARRAY);
	}

	/** The sql.raw param on top of the stack: strings as text, integers as integers, numbers as reals, and booleans. */
	private parameter(L: LuaState): string | bigint | number | boolean {
		const { lua } = this;
		const type = lua.lua_type(L, -1);

		if (type === LuaType.String) return lua.lua_tolstring(L, -1, null);

		if (type === LuaType.Number) {
			return lua.lua_isinteger(L, -1) ? lua.lua_tointegerx(L, -1, null) : lua.lua_tonumberx(L, -1, null);
		}

		if (type === LuaType.Boolean) return lua.lua_toboolean(L, -1) !== 0;

		throw new Error(`unsupported parameter type: ${lua.lua_typename(L, type)}`);
	}

	/**
	 * Pushes `value` the way mlua's `to_value` does: arrays get the array metatable (so they encode as arrays even
	 * when empty) and null becomes a null light userdata. JSON.parse can't tell 1.0 from 1, but atproto data has no
	 * floats, so whole numbers become integers.
	 */
	private push(L: LuaState, value: JsonValue): void {
		const { lua } = this;

		if (value === null) return lua.lua_pushlightuserdata(L, 0);

		if (value === true || value === false) return lua.lua_pushboolean(L, value ? 1 : 0);

		if (isText(value)) return void lua.lua_pushstring(L, value);

		if (isNumber(value)) {
			return Number.isSafeInteger(value) ? lua.lua_pushinteger(L, BigInt(value)) : lua.lua_pushnumber(L, value);
		}

		lua.luaL_checkstack(L, 2, null);

		if (Array.isArray(value)) {
			lua.lua_createtable(L, value.length, 0);
			value.forEach((item, i) => {
				this.push(L, item);
				lua.lua_rawseti(L, -2, BigInt(i + 1));
			});

			return lua.luaL_setmetatable(L, ARRAY);
		}

		lua.lua_createtable(L, 0, 0);

		for (const [key, item] of Object.entries(value)) {
			if (item === undefined) continue;
			this.push(L, item);
			lua.lua_setfield(L, -2, key);
		}
	}

	/**
	 * Converts the value at `index` the way mlua's `from_value` does with HappyView's options: a table with the
	 * array metatable or a nonzero length is an array, any other table an object with string keys.
	 */
	private toJson(L: LuaState, index: number, visiting = new Set<number>()): JsonValue {
		const { lua } = this;
		const at = lua.lua_absindex(L, index);
		const type = lua.lua_type(L, at);

		if (type === LuaType.Nil) return null;

		if (type === LuaType.Boolean) return lua.lua_toboolean(L, at) !== 0;

		if (type === LuaType.String) return lua.lua_tolstring(L, at, null);

		if (type === LuaType.Number) {
			if (lua.lua_isinteger(L, at)) return Number(lua.lua_tointegerx(L, at, null));
			const number = lua.lua_tonumberx(L, at, null);

			return Number.isFinite(number) ? number : null;
		}

		if (type === LuaType.LightUserdata && lua.lua_touserdata(L, at) === 0) return null;

		if (type !== LuaType.Table) throw new Error(`unsupported value type \`${lua.lua_typename(L, type)}\``);
		const pointer = lua.lua_topointer(L, at);

		if (visiting.has(pointer)) throw new Error('recursive table detected');
		visiting.add(pointer);
		const length = lua.lua_rawlen(L, at);
		let out: JsonValue;

		if (length > 0 || this.isArray(L, at)) {
			out = [];

			for (let i = 1; i <= length; i++) {
				lua.lua_rawgeti(L, at, BigInt(i));
				out.push(this.toJson(L, -1, visiting));
				lua.lua_settop(L, -2);
			}
		} else {
			out = {};
			lua.lua_pushnil(L);

			while (lua.lua_next(L, at) !== 0) {
				if (lua.lua_type(L, -2) !== LuaType.String) throw new Error('object keys must be strings');
				out[lua.lua_tolstring(L, -2, null)] = this.toJson(L, -1, visiting);
				lua.lua_settop(L, -2);
			}
		}

		visiting.delete(pointer);

		return out;
	}

	private isArray(L: LuaState, at: number): boolean {
		const { lua } = this;

		if (!lua.lua_getmetatable(L, at)) return false;
		lua.luaL_getmetatable(L, ARRAY);
		const marked = lua.lua_rawequal(L, -1, -2) !== 0;
		lua.lua_settop(L, -3);

		return marked;
	}
}

let engine: Promise<Engine> | undefined;

/** One Postgres for every FakeHappyView in the process. */
let postgres: Promise<PGlite> | undefined;

/** A freshly made Postgres data directory, which every test file's Postgres starts from. */
const DATA_DIR = join(tmpdir(), 'localnetwork-pglite-data.tar');

/**
 * Postgres, from the data directory a test file made before, since making one (initdb) takes about half a second and
 * loading it a tenth of that. Without one, or with one this PGlite can't start from, it makes its own and saves it.
 */
async function startPostgres(): Promise<PGlite> {
	try {
		return await PGlite.create({ loadDataDir: new Blob([await readFile(DATA_DIR)]) });
	} catch {
		const pg = await PGlite.create();
		const saving = `${DATA_DIR}.${process.pid}`;

		// Written aside, then moved into place, so a file starting at the same time never reads half of it.
		await writeFile(saving, (await pg.dumpDataDir('none')).stream());
		await rename(saving, DATA_DIR);

		return pg;
	}
}

let schemas = 0;

type Param = LexXrpcParameters['properties'][string];

/** A param's text as the type the lexicon gives it, the way HappyView 3's coerce_param reads it. */
function coerce(
	key: string,
	value: string | string[],
	def: Param | Extract<Param, { type: 'array' }>['items']
): JsonValue {
	if (def.type === 'array') return [value].flat().map((item) => coerce(key, item, def.items));

	// A repeated param the lexicon doesn't declare as an array stays a list, which the lexicon check then refuses.
	if (Array.isArray(value)) return value;

	if (def.type === 'integer') {
		if (!/^[+-]?\d+$/.test(value)) throw new ValidationError(`${key} must be an integer`);

		return Number(value);
	}

	if (def.type !== 'boolean') return value;

	if (value === 'true' || value === '1') return true;

	if (value === 'false' || value === '0') return false;
	throw new ValidationError(`${key} must be a boolean`);
}

/**
 * The input HappyView 3 gives a query script: its parse_query_params (a `+` reads as a space, a key given once is a
 * string and a repeated one a list of strings), then each param the lexicon declares coerced to its type (an array
 * even when given once) and checked against the lexicon, with defaults filled in. Throws a ValidationError for params
 * the lexicon refuses, which HappyView answers with a 400.
 */
function readParams(query: string, nsid: string, schema: LexXrpcParameters | undefined, lexicons: Lexicons) {
	const decode = (text: string) => {
		try {
			return decodeURIComponent(text.replaceAll('+', ' '));
		} catch {
			return '';
		}
	};

	const values = new Map<string, string[]>();

	for (const pair of query.split('&')) {
		if (!pair) continue;
		const split = pair.indexOf('=');
		const key = decode(split < 0 ? pair : pair.slice(0, split));
		values.set(key, [...(values.get(key) ?? []), split < 0 ? '' : decode(pair.slice(split + 1))]);
	}

	const params = Object.fromEntries(
		values.entries().map(([key, list]) => {
			const value = list.length === 1 ? list[0] : list;
			const def = schema?.properties[key];

			return [key, def ? coerce(key, value, def) : value];
		})
	);

	// SAFETY: the lexicon check only fills in defaults, which are JSON too.
	return (lexicons.assertValidXrpcParams(nsid, params) ?? params) as Record<string, JsonValue>;
}

/**
 * A schema of its own in the shared Postgres, made on its first query (most tests never make one, and starting
 * Postgres takes a good part of a second), which every statement runs in. A transaction per statement keeps another
 * instance's statements out of its search path.
 */
function database(): Database {
	const schema = `fake_${(schemas += 1)}`;
	let ready: Promise<PGlite> | undefined;

	const start = async () => {
		const pg = await (postgres ??= startPostgres());

		await pg.transaction(async (tx) => {
			await tx.exec(`CREATE SCHEMA ${schema}; SET LOCAL search_path TO ${schema}; ${SCHEMA}`);
		});

		return pg;
	};

	return {
		async query(statement, params) {
			const pg = await (ready ??= start());

			return pg.transaction(async (tx) => {
				await tx.exec(`SET LOCAL search_path TO ${schema}`);

				return tx.query<(string | null)[]>(statement, params, { rowMode: 'array', parsers: AS_TEXT });
			});
		}
	};
}

/** POST /admin/backfill's body, for one collection: up to 500 accounts in `dids`, and `did` added to them. */
const BACKFILL = z.object({
	collection: z.string(),
	dids: z.array(z.string()).max(500).optional(),
	did: z.string().optional()
});

/** POST /admin/network-lexicons' body. */
const NETWORK_LEXICON = z.object({ nsid: z.string(), indexed: z.boolean().optional(), backfill: z.boolean() });

/** A Jetstream commit as HappyView sees it. `record` is null for a delete. */
export type RecordEvent = { did: string; collection: string; rkey: string; cid: string; record: JsonRecord | null };

/**
 * An in-process HappyView for unit tests. Everything under test is the real bundled Lua; this stands in for HappyView's
 * Rust plumbing only, as of 3.0.0-beta.14: refusing scripts that still read v2 globals and checking them the way an
 * upload does, query param parsing and lexicon checks, handle(input, ctx) and the modules the scripts `require` (with
 * `web` answering their HTTP requests), record script outcomes, the instruction limit, and backfills of a list of
 * accounts. Raw SQL runs on Postgres (PGlite) the way the hosted instance's record store answers it: the records view
 * with `uri` computed from did, collection and rkey and `record` as text, the tables raw SQL may name, no query mixing
 * the record views with other tables, params only as an array, and columns read as sqlx reads them. It doesn't check
 * CIDs, domains, or auth, retry a failing record script, time queries out (the store fails one that isn't keyed by
 * DIDs), or type params the way HappyView binds them (Postgres infers each from the SQL here).
 */
export class FakeHappyView {
	/** An account's records of a collection as its PDS lists them, for backfills; without it, there are none. */
	repo?: (did: string, collection: string) => RecordEvent[];
	/** Backfill jobs' statuses by id, in the order they started. */
	private readonly jobs = new Map<string, string>();
	/** Lua instructions the last handle() call used, rounded down to a thousand. The budget is 1,000,000. */
	instructions = 0;
	/** Messages scripts logged through `internal.logging`, at any level. */
	readonly logs: string[] = [];
	/** What answers scripts' HTTP requests; without it, nothing does. Nothing a script asks for reaches the network. */
	web?: Web;
	/** The URLs scripts asked for, in order. */
	readonly requests: string[] = [];
	private readonly db: Database;
	private readonly lexicons: Map<string, LexiconDoc>;
	private readonly checker: Lexicons;
	/** The other authorities' lexicons it tracks from the network, and whether it ingests each one's records. */
	private readonly network: Map<string, boolean>;
	private readonly scripts: Map<string, string>;

	/**
	 * `bundle` replaces the app's bundle, to test this plumbing with other scripts. Throws, as HappyView refuses the
	 * upload, when a script still reads a global v3 removed.
	 */
	constructor(rootDid: string, bundle: HappyViewBundle = happyViewBundle(rootDid)) {
		for (const { id, body } of bundle.scripts) {
			const removed = removedGlobals(id, body);

			if (removed.length) throw new Error(`${id}: script references removed globals: ${removed.join(', ')}`);
		}

		// It starts as a sync leaves it, tracking the bundle's network lexicons, as the installed set has them.
		this.network = new Map(bundle.network.map(({ nsid, indexed }) => [nsid, indexed]));
		const tracked = REFERENCED_DOCS.filter((doc) => this.network.has(doc.id));
		this.lexicons = new Map([...tracked, ...bundle.lexicons].map((doc) => [doc.id, doc]));
		// Lexicons rewrites the refs in the docs it's given, which the bundle shares with every other caller.
		this.checker = new Lexicons(structuredClone([...this.lexicons.values()]));
		this.scripts = new Map(bundle.scripts.map((script) => [script.id, script.body]));
		this.db = database();
	}

	/** What HappyView's Jetstream consumer does with an event: run the record script, then store or delete. */
	async ingest(event: RecordEvent): Promise<void> {
		const { did, collection, rkey, cid, record } = event;

		if (this.lexicons.get(collection)?.defs.main?.type !== 'record' || this.network.get(collection) === false) return;
		const uri = `at://${did}/${collection}/${rkey}`;

		const found = await this.db.query(
			'SELECT 1 FROM happyview_records WHERE did = $1 AND collection = $2 AND rkey = $3',
			[did, collection, rkey]
		);

		const exists = found.rows.length > 0;

		const action = !record ? 'delete' : exists ? 'update' : 'create';
		const trigger = [`record.${action}:${collection}`, `record.index:${collection}`].find((id) => this.scripts.has(id));
		let stored: JsonValue = record;

		if (trigger) {
			try {
				// A delete has no record, rather than a null one.
				const commit = { action, uri, did, collection, rkey };
				const input = record ? { ...commit, record } : commit;
				const ctx = { trigger, caller_did: did, has_pds_auth: false, env: {}, collection };
				const result = await this.execute(trigger, { input, ctx });

				if (result.type === LuaType.Nil) return;

				if (result.type === LuaType.Table) stored = result.value;
			} catch (error) {
				// HappyView retries a failing script a few times, then indexes the event as if there were none.
				if (!(error instanceof ScriptError)) throw error;
			}
		}

		if (!record) {
			await this.db.query(
				`DELETE FROM happyview_records_v2 v USING happyview_dids d, happyview_collections c
				WHERE v.did_id = d.id AND v.collection_id = c.id AND d.did = $1 AND c.nsid = $2 AND v.rkey = $3`,
				[did, collection, rkey]
			);

			return;
		}

		await this.store([{ did, collection, rkey, cid, record: stored }]);
	}

	/**
	 * Stores records as they are, without running their record script, for tests that need many records the script
	 * would keep unchanged and only measure the queries over them.
	 */
	seed(records: { did: string; collection: string; rkey: string; cid: string; record: JsonRecord }[]): Promise<void> {
		return this.store(records);
	}

	/**
	 * Serves `GET /xrpc/<nsid>` from the query's script, the way HappyView does, and with `repo`, the backfill admin
	 * calls.
	 */
	readonly fetch: typeof globalThis.fetch = async (input, init) => {
		const request = new Request(input, init);
		const url = new URL(request.url);

		if (this.repo && url.pathname === '/admin/backfill' && request.method === 'POST')
			return Response.json(await this.backfill(BACKFILL.parse(await request.json())), { status: 201 });

		if (this.repo && url.pathname === '/admin/backfill/status')
			return Response.json([...this.jobs].map(([id, status]) => ({ id, status })));

		if (url.pathname === '/admin/network-lexicons') return this.networkLexicons(request);
		const nsid = /^\/xrpc\/([^/]+)$/.exec(url.pathname)?.[1] ?? '';
		const main = this.lexicons.get(nsid)?.defs.main;
		const trigger = `xrpc.query:${nsid}`;

		if (request.method !== 'GET' || main?.type !== 'query' || !this.scripts.has(trigger)) {
			return Response.json({ error: 'MethodNotImplemented', message: `${nsid} is not served here` }, { status: 404 });
		}

		try {
			const params = readParams(url.search.slice(1), nsid, main.parameters, this.checker);
			const ctx = { trigger, has_pds_auth: false, env: {}, method: nsid };

			return Response.json((await this.execute(trigger, { input: params, ctx })).value);
		} catch (error) {
			if (error instanceof ValidationError)
				return Response.json({ error: 'InvalidRequest', message: error.message }, { status: 400 });

			if (!(error instanceof ScriptError)) throw error;

			return Response.json(
				{ error: 'script_error', errorType: error.errorType, message: error.message, method: nsid },
				{ status: error.errorType === 'timeout' ? 408 : 500 }
			);
		}
	};

	/** Upserts records the way HappyView writes one it indexed: a new one gets its created_at, a changed one keeps it. */
	private async store(records: { did: string; collection: string; rkey: string; cid: string; record: JsonValue }[]) {
		const column = (read: (record: (typeof records)[number]) => string) => records.map(read);

		await this.db.query(
			`WITH dids AS (INSERT INTO happyview_dids (did) SELECT DISTINCT unnest($1::text[]) ON CONFLICT DO NOTHING),
			collections AS (INSERT INTO happyview_collections (nsid) SELECT DISTINCT unnest($2::text[]) ON CONFLICT DO NOTHING)
			SELECT 1`,
			[column(({ did }) => did), column(({ collection }) => collection)]
		);
		await this.db.query(
			`INSERT INTO happyview_records_v2 (did_id, collection_id, rkey, cid, body, indexed_at)
			SELECT d.id, c.id, i.rkey, i.cid, i.body, now()
			FROM unnest($1::text[], $2::text[], $3::text[], $4::text[], $5::text[]) AS i (did, collection, rkey, cid, body)
			JOIN happyview_dids d ON d.did = i.did
			JOIN happyview_collections c ON c.nsid = i.collection
			ON CONFLICT (did_id, collection_id, rkey) DO UPDATE
			SET body = excluded.body, cid = excluded.cid, indexed_at = excluded.indexed_at`,
			[
				column(({ did }) => did),
				column(({ collection }) => collection),
				column(({ rkey }) => rkey),
				column(({ cid }) => cid),
				column(({ record }) => JSON.stringify(record))
			]
		);
	}

	/**
	 * `GET` and `POST /admin/network-lexicons`: the network lexicons it tracks, or one more to track. The network is the
	 * installed set (lexicons/network/): an NSID outside it fails, as one HappyView can't resolve does.
	 */
	private async networkLexicons(request: Request): Promise<Response> {
		if (request.method === 'GET')
			return Response.json([...this.network].map(([nsid, indexed]) => ({ nsid, indexed, declared: false })));

		const { nsid, indexed = true } = NETWORK_LEXICON.parse(await request.json());
		const doc = REFERENCED_DOCS.find((lexicon) => lexicon.id === nsid);

		if (!doc) return Response.json({ error: 'BadRequest', message: `could not resolve ${nsid}` }, { status: 400 });
		this.network.set(nsid, indexed);
		this.lexicons.set(nsid, doc);

		if (!this.checker.get(nsid)) this.checker.add(structuredClone(doc));

		return Response.json({ nsid, authority_did: 'did:plc:authority', revision: 1 }, { status: 201 });
	}

	/**
	 * Re-reads the named accounts' records of `collection` through its record script, as one job, then answers the
	 * way HappyView does once it has queued the job.
	 */
	private async backfill({ collection, dids = [], did }: z.infer<typeof BACKFILL>) {
		const id = `job-${this.jobs.size}`;
		this.jobs.set(id, 'running');

		// HappyView adds `did` to `dids` and ignores duplicates.
		for (const account of new Set(did === undefined ? dids : [...dids, did]))
			for (const event of this.repo?.(account, collection) ?? []) await this.ingest(event);
		this.jobs.set(id, 'completed');

		return { id, status: 'running' };
	}

	private async execute(trigger: string, call: Call): Promise<ScriptResult> {
		engine ??= new LuaFactory().getLuaModule().then((lua) => new Engine(lua));
		const running = await engine;

		const body = this.scripts.get(trigger) ?? '';
		// A script's first run stands in for its upload, which checks it. Checking only the scripts a test runs keeps
		// a test that runs one from loading every query.
		await running.upload(this.db, trigger, body);

		try {
			const web: Web = (url) => {
				this.requests.push(url);

				return this.web?.(url);
			};

			return await running.execute(this.db, this.logs, web, trigger, body, call);
		} finally {
			this.instructions = running.instructions;
		}
	}
}
