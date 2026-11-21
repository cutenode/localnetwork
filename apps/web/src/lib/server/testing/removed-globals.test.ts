import { describe, expect, it } from 'vitest';
import { removedGlobals } from './removed-globals';

// The cases HappyView 3's own needs_migration tests cover (src/codemod/detect.rs).
describe('removedGlobals', () => {
	it('names every free removed global in a fixed order', () => {
		expect(removedGlobals('xrpc.query:a.b.c', 'function handle() log(now()) return db.get(params.uri) end')).toEqual([
			'db',
			'log',
			'now',
			'params'
		]);
	});

	it('passes a migrated script', () => {
		const body = `local log = require('internal.logging')
local db = require('happyview.db')
function handle(input, ctx)
	log.info('x')
	return db.records(ctx.collection):run()
end`;

		expect(removedGlobals('xrpc.query:a.b.c', body)).toEqual([]);
	});

	it("leaves out the script's own locals, and globals it assigns", () => {
		expect(removedGlobals('record.index:a.b.c', 'function handle() local record = {} return record.title end')).toEqual(
			[]
		);
		expect(removedGlobals('xrpc.query:a.b.c', 'json = {} function handle() return json end')).toEqual([]);
		expect(
			removedGlobals(
				'xrpc.query:a.b.c',
				'local rows = db.get(1) for _, db in ipairs(rows) do end function handle() end'
			)
		).toEqual(['db']);
	});

	it('counts log only where it is called', () => {
		expect(removedGlobals('xrpc.query:a.b.c', 'function handle() return { log = log } end')).toEqual([]);
		expect(removedGlobals('xrpc.query:a.b.c', "function handle() log 'x' end")).toEqual(['log']);
	});

	it('counts a name only for the kinds of script it was set for', () => {
		const body = 'function handle() return { rkey = rkey } end';

		expect(removedGlobals('record.index:a.b.c', body)).toEqual(['rkey']);
		expect(removedGlobals('xrpc.query:a.b.c', body)).toEqual([]);
		expect(removedGlobals('labeler.apply:_actor', body)).toEqual(['rkey']);
	});

	it("leaves a script it can't parse to the load", () => {
		expect(removedGlobals('xrpc.query:a.b.c', 'function handle( end')).toEqual([]);
	});
});
