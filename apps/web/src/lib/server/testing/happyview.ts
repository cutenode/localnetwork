import { DatabaseSync, type SQLInputValue } from 'node:sqlite';
import type { LexiconDoc, LexXrpcParameters } from '@atproto/lexicon';
import type { JsonValue } from '@atproto/lex-json';
import { LuaEventMasks, LuaFactory, LuaReturn, type LuaState, LuaType, type LuaWasm } from 'wasmoon';
import type { JsonRecord } from '../atproto/repo-client';
import { type HappyViewBundle, happyViewBundle } from '../happyview/bundle';

/**
 * HappyView's sandbox stops a script after this many Lua instructions. In 2.16 its hook only reaches a script's
 * top-level code (also run when the script is uploaded): handle() runs in a coroutine the hook misses. This holds
 * handle() to the limit too, so the scripts stay within the budget HappyView means to enforce, and a runaway loop
 * fails a test instead of hanging it.
 */
const INSTRUCTION_LIMIT = 1_000_000;

/** How often the count hook here fires, which is also how precisely `instructions` counts. */
const HOOK_INTERVAL = 1_000;

/** Registry name of the metatable that marks a table as a JSON array, like mlua's array metatable. */
const ARRAY = 'array';

/** What HappyView's sandbox leaves of Lua's standard library. */
const SANDBOX = `local system = os
os = { time = system.time, date = system.date, difftime = system.difftime, clock = system.clock }
io, debug, package, require, dofile, loadfile, load, collectgarbage = nil`;

/** HappyView's records table (SQLite flavor), the one table the scripts read. */
const SCHEMA = `CREATE TABLE happyview_records (
	uri TEXT PRIMARY KEY,
	did TEXT NOT NULL,
	collection TEXT NOT NULL,
	rkey TEXT NOT NULL,
	record TEXT NOT NULL,
	cid TEXT NOT NULL,
	indexed_at TEXT DEFAULT (datetime('now')),
	created_at TEXT NOT NULL DEFAULT (datetime('now'))
)`;

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

type Globals = Record<string, JsonValue | undefined>;

// What's left of a JSON or SQLite value once the other kinds are ruled out.
const isText = <T>(value: T): value is Extract<T, string> => value === String(value);

const isNumber = <T>(value: T): value is Extract<T, number> => value === Number(value);

/**
 * Lua 5.4 (wasmoon) set up the way HappyView's Rust runner sets up mlua: a fresh state per run, its sandbox and
 * globals, `db.raw` over SQLite, `json`, `toarray`, `log`, and the instruction limit.
 */
class Engine {
	/** Lua instructions the last run used, counted in steps of `HOOK_INTERVAL`. */
	instructions = 0;
	/** The run in progress. Runs are synchronous from start to finish, so there is never more than one. */
	private run?: { db: DatabaseSync; logs: string[]; ticks: number };
	private readonly hook: number;
	/** C functions HappyView sets as globals, and the global tables it puts the rest in. */
	private readonly functions: Record<string, number>;
	private readonly tables: Record<string, Record<string, number>>;

	constructor(private readonly lua: LuaWasm) {
		this.hook = lua.module.addFunction((L: LuaState) => {
			const run = this.current();
			run.ticks += 1;

			if (run.ticks * HOOK_INTERVAL < INSTRUCTION_LIMIT) return;
			lua.lua_pushstring(L, 'script exceeded execution limit');
			lua.lua_error(L);
		}, 'vii');

		this.tables = {
			db: { raw: this.cFunction('db.raw', (L) => this.raw(L)) },
			json: {
				encode: this.cFunction('json.encode', (L) => {
					lua.lua_pushstring(L, JSON.stringify(this.toJson(L, 1)));

					return 1;
				}),
				decode: this.cFunction('json.decode', (L) => {
					this.push(L, JSON.parse(lua.luaL_checklstring(L, 1, null)));

					return 1;
				})
			}
		};

		this.functions = {
			toarray: this.cFunction('toarray', (L) => {
				lua.luaL_checktype(L, 1, LuaType.Table);
				lua.lua_createtable(L, 0, 0);

				for (let i = 1n; lua.lua_rawgeti(L, 1, i) !== LuaType.Nil; i++) lua.lua_rawseti(L, -2, i);
				lua.lua_settop(L, -2);
				lua.luaL_setmetatable(L, ARRAY);

				return 1;
			}),
			log: this.cFunction('log', (L) => {
				this.current().logs.push(lua.luaL_checklstring(L, 1, null));

				return 0;
			})
		};
	}

	/**
	 * Loads `body` into a fresh state with `globals` set (nil when undefined) and calls its handle(). Loading and
	 * handle() each get the full instruction budget.
	 */
	execute(db: DatabaseSync, logs: string[], name: string, body: string, globals: Globals): ScriptResult {
		const { lua } = this;
		const L = lua.luaL_newstate();
		const run = { db, logs, ticks: 0 };
		this.run = run;

		try {
			lua.luaL_openlibs(L);
			lua.luaL_loadstring(L, SANDBOX);
			lua.lua_callk(L, 0, 0, 0, null);
			lua.luaL_newmetatable(L, ARRAY);
			lua.lua_settop(L, 0);

			for (const [global, pointer] of Object.entries(this.functions)) {
				lua.lua_pushcclosure(L, pointer, 0);
				lua.lua_setglobal(L, global);
			}

			for (const [global, functions] of Object.entries(this.tables)) {
				lua.lua_createtable(L, 0, 0);

				for (const [field, pointer] of Object.entries(functions)) {
					lua.lua_pushcclosure(L, pointer, 0);
					lua.lua_setfield(L, -2, field);
				}

				lua.lua_setglobal(L, global);
			}

			for (const [global, value] of Object.entries(globals)) {
				if (value === undefined) continue;
				this.push(L, value);
				lua.lua_setglobal(L, global);
			}

			lua.lua_sethook(L, this.hook, LuaEventMasks.Count, HOOK_INTERVAL);

			const loaded =
				lua.luaL_loadbufferx(L, body, lua.module.lengthBytesUTF8(body), `=${name}`, null) === LuaReturn.Ok
					? this.call(L, 0)
					: lua.lua_tolstring(L, -1, null);

			if (loaded !== undefined) throw new ScriptError('syntax', loaded);
			run.ticks = 0;
			lua.lua_sethook(L, this.hook, LuaEventMasks.Count, HOOK_INTERVAL);

			if (lua.lua_getglobal(L, 'handle') !== LuaType.Function) {
				throw new ScriptError('missing_handle', 'script does not define a handle() function');
			}

			const failed = this.call(L, 1);

			if (failed?.includes('execution limit')) throw new ScriptError('timeout', 'script exceeded execution time limit');

			if (failed !== undefined) throw new ScriptError('runtime', failed);

			try {
				return { type: lua.lua_type(L, -1), value: this.toJson(L, -1) };
			} catch (error) {
				throw new ScriptError('runtime', error instanceof Error ? error.message : String(error));
			}
		} finally {
			this.instructions = run.ticks * HOOK_INTERVAL;
			this.run = undefined;
			lua.lua_close(L);
		}
	}

	private current() {
		if (!this.run) throw new Error('No script is running');

		return this.run;
	}

	/** Calls the function on top of the stack, keeping `results` of its results. Returns the error, if any. */
	private call(L: LuaState, results: number): string | undefined {
		// 0 is LUA_OK; wasmoon types the status as a plain number.
		if (this.lua.lua_pcallk(L, 0, results, 0, 0, null) === 0) return undefined;

		return this.lua.luaL_tolstring(L, -1, null);
	}

	/** A Lua C function that runs `body`, which returns how many results it pushed. JS errors become Lua errors. */
	private cFunction(name: string, body: (L: LuaState) => number): number {
		return this.lua.module.addFunction((L: LuaState) => {
			try {
				return body(L);
			} catch (error) {
				// Emscripten unwinds a Lua error (a longjmp) by throwing Infinity, which has to pass through.
				if (error === Infinity) throw error;
				this.lua.lua_pushstring(L, `${name}: ${error instanceof Error ? error.message : String(error)}`);
			}

			return this.lua.lua_error(L);
		}, 'ii');
	}

	/** `db.raw(sql, params)`: binds like HappyView and converts rows like its sqlx `try_get` chain. */
	private raw(L: LuaState): number {
		const { lua } = this;
		const sql = lua.luaL_checklstring(L, 1, null);
		const values: SQLInputValue[] = [];

		// A Lua sequence ends at its first nil, so a nil param ends the list.
		if (lua.lua_type(L, 2) === LuaType.Table) {
			for (let i = 1n; lua.lua_rawgeti(L, 2, i) !== LuaType.Nil; i++) {
				values.push(this.parameter(L));
				lua.lua_settop(L, -2);
			}
		}

		const statement = this.current().db.prepare(sql);
		// Integers come back as bigints, so they stay apart from reals.
		statement.setReadBigInts(true);
		const rows = statement.all(...values);
		lua.lua_createtable(L, rows.length, 0);

		rows.forEach((row, i) => {
			lua.lua_createtable(L, 0, 0);

			for (const [column, value] of Object.entries(row)) {
				// sqlx tries text, then integer, then real. NULL and blobs fail every try and come out nil.
				if (value === null || value instanceof Uint8Array) continue;

				if (isText(value)) lua.lua_pushstring(L, value);
				else if (isNumber(value)) lua.lua_pushnumber(L, value);
				else lua.lua_pushinteger(L, value);
				lua.lua_setfield(L, -2, column);
			}

			lua.lua_rawseti(L, -2, BigInt(i + 1));
		});
		lua.luaL_setmetatable(L, ARRAY);

		return 1;
	}

	/** The db.raw param on top of the stack: strings as text, integers as integers, numbers as reals, booleans as 1/0. */
	private parameter(L: LuaState): SQLInputValue {
		const { lua } = this;
		const type = lua.lua_type(L, -1);

		if (type === LuaType.String) return lua.lua_tolstring(L, -1, null);

		if (type === LuaType.Number) {
			return lua.lua_isinteger(L, -1) ? lua.lua_tointegerx(L, -1, null) : lua.lua_tonumberx(L, -1, null);
		}

		if (type === LuaType.Boolean) return lua.lua_toboolean(L, -1);

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

/** HappyView's coerce_params for a param given once: an integer or boolean where the lexicon says so and it parses. */
function coerce(value: string, type: string | undefined): JsonValue {
	if (type === 'integer' && /^[+-]?\d+$/.test(value)) return Number(value);

	if (type === 'boolean' && (value === 'true' || value === '1')) return true;

	if (type === 'boolean' && (value === 'false' || value === '0')) return false;

	return value;
}

/**
 * HappyView's parse_query_params, then coerce_params: a key given once maps to a string (coerced), a repeated key to
 * a list of strings. A `+` stays a plus.
 */
function readParams(query: string, schema: LexXrpcParameters | undefined): Record<string, JsonValue> {
	const decode = (text: string) => {
		try {
			return decodeURIComponent(text);
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

	return Object.fromEntries(
		values
			.entries()
			.map(([key, list]) => [key, list.length === 1 ? coerce(list[0], schema?.properties[key]?.type) : list])
	);
}

/** A Jetstream commit as HappyView sees it. `record` is null for a delete. */
export type RecordEvent = { did: string; collection: string; rkey: string; cid: string; record: JsonRecord | null };

/**
 * An in-process HappyView for unit tests. Everything under test is the real bundled Lua; this stands in for
 * HappyView's Rust plumbing only, as of 2.16: param parsing and coercion, the script globals and APIs, record script
 * outcomes, and the instruction limit, over node:sqlite. It doesn't check CIDs, domains, or auth.
 */
export class FakeHappyView {
	/** Lua instructions the last handle() call used, rounded down to a thousand. The budget is 1,000,000. */
	instructions = 0;
	/** Messages scripts passed to `log()`. */
	readonly logs: string[] = [];
	private readonly db = new DatabaseSync(':memory:');
	private readonly lexicons: Map<string, LexiconDoc>;
	private readonly scripts: Map<string, string>;

	/** `bundle` replaces the app's bundle, to test this plumbing with other scripts. */
	constructor(rootDid: string, bundle: HappyViewBundle = happyViewBundle(rootDid)) {
		this.lexicons = new Map(bundle.lexicons.map((doc) => [doc.id, doc]));
		this.scripts = new Map(bundle.scripts.map((script) => [script.id, script.body]));
		this.db.exec(SCHEMA);
	}

	/** What HappyView's Jetstream consumer does with an event: run the record script, then store or delete. */
	async ingest(event: RecordEvent): Promise<void> {
		const { did, collection, rkey, cid, record } = event;

		if (this.lexicons.get(collection)?.defs.main?.type !== 'record') return;
		const uri = `at://${did}/${collection}/${rkey}`;
		const exists = this.db.prepare('SELECT 1 FROM happyview_records WHERE uri = ?').get(uri) !== undefined;
		const action = !record ? 'delete' : exists ? 'update' : 'create';
		const trigger = [`record.${action}:${collection}`, `record.index:${collection}`].find((id) => this.scripts.has(id));
		let stored: JsonValue = record;

		if (trigger) {
			try {
				const context = { action, uri, did, collection, rkey, record };

				const result = await this.execute(trigger, {
					...context,
					record: record ?? undefined,
					event: context,
					env: {}
				});

				if (result.type === LuaType.Nil) return;

				if (result.type === LuaType.Table) stored = result.value;
			} catch (error) {
				// HappyView retries a failing script a few times, then indexes the event as if there were none.
				if (!(error instanceof ScriptError)) throw error;
			}
		}

		if (!record) {
			this.db.prepare('DELETE FROM happyview_records WHERE uri = ?').run(uri);

			return;
		}

		const now = new Date().toISOString();
		this.db
			.prepare(
				`INSERT INTO happyview_records (uri, did, collection, rkey, record, cid, indexed_at, created_at)
				VALUES (?, ?, ?, ?, ?, ?, ?, ?)
				ON CONFLICT (uri) DO UPDATE SET record = excluded.record, cid = excluded.cid, indexed_at = ?`
			)
			.run(uri, did, collection, rkey, JSON.stringify(stored), cid, now, now, now);
	}

	/**
	 * Stores records as they are, without running their record script, for tests that need many records the script
	 * would keep unchanged and only measure the queries over them.
	 */
	seed(records: { did: string; collection: string; rkey: string; cid: string; record: JsonRecord }[]): void {
		const now = new Date().toISOString();

		const insert = this.db.prepare(
			`INSERT INTO happyview_records (uri, did, collection, rkey, record, cid, indexed_at, created_at)
			VALUES (?, ?, ?, ?, ?, ?, ?, ?)`
		);

		this.db.exec('BEGIN');

		for (const { did, collection, rkey, cid, record } of records)
			insert.run(`at://${did}/${collection}/${rkey}`, did, collection, rkey, JSON.stringify(record), cid, now, now);
		this.db.exec('COMMIT');
	}

	/** Serves `GET /xrpc/<nsid>` from the query's script, the way HappyView does. */
	readonly fetch: typeof globalThis.fetch = async (input, init) => {
		const request = new Request(input, init);
		const url = new URL(request.url);
		const nsid = /^\/xrpc\/([^/]+)$/.exec(url.pathname)?.[1] ?? '';
		const main = this.lexicons.get(nsid)?.defs.main;
		const trigger = `xrpc.query:${nsid}`;

		if (request.method !== 'GET' || main?.type !== 'query' || !this.scripts.has(trigger)) {
			return Response.json({ error: 'MethodNotImplemented', message: `${nsid} is not served here` }, { status: 404 });
		}

		const params = readParams(url.search.slice(1), main.parameters);

		try {
			const result = await this.execute(trigger, { method: nsid, params, collection: '', env: {} });

			return Response.json(result.value);
		} catch (error) {
			if (!(error instanceof ScriptError)) throw error;

			return Response.json(
				{ error: 'script_error', errorType: error.errorType, message: error.message, method: nsid },
				{ status: error.errorType === 'timeout' ? 408 : 500 }
			);
		}
	};

	private async execute(trigger: string, globals: Globals): Promise<ScriptResult> {
		engine ??= new LuaFactory().getLuaModule().then((lua) => new Engine(lua));
		const running = await engine;

		try {
			return running.execute(this.db, this.logs, trigger, this.scripts.get(trigger) ?? '', globals);
		} finally {
			this.instructions = running.instructions;
		}
	}
}
