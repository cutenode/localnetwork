import type { LexiconDoc } from '@atproto/lexicon';
import { GENRE_ALIASES, GENRE_FAMILIES } from '#lib/taxonomy/genres.ts';
import { FEE_TIER_ORDER } from '#lib/taxonomy/options.ts';
import { LUA_FILES } from '#lua-files';
import { LEXICON_DOCS, REFERENCED_DOCS } from '../atproto/lexicons';

/** A HappyView script, keyed by its trigger, e.g. `xrpc.query:computer.localnetwork.directory.searchProjects`. */
export type HappyViewScript = { id: string; body: string };

/** Everything the app uploads to HappyView. */
export type HappyViewBundle = { lexicons: LexiconDoc[]; scripts: HappyViewScript[] };

const LIBRARY = '/lua/lib/';

/** `text` as a Lua long string, with enough `=` in its brackets that nothing inside can close it early. */
function longString(text: string): string {
	let level = '';

	while (`${text}]`.includes(`]${level}]`)) level += '=';

	// Lua drops a newline right after the opening bracket, so one goes there to keep a leading newline in `text`.
	return `[${level}[\n${text}]${level}]`;
}

/** `items` as a Lua list of long strings. */
const luaList = (items: readonly string[]) => `{ ${items.map(longString).join(', ')} }`;

/** The genre and fee taxonomies as a Lua table, with the fields the scripts read. */
function luaTaxonomy(): string {
	const families = GENRE_FAMILIES.map(
		({ id, genres, near = [] }) => `{ id = ${longString(id)}, genres = ${luaList(genres)}, near = ${luaList(near)} }`
	);

	// The spaces keep `[` and the long string's own `[[` from reading as one bracket.
	const aliases = [...GENRE_ALIASES].map(([alias, genre]) => `[ ${longString(alias)} ] = ${longString(genre)}`);

	return `{ families = { ${families.join(', ')} }, aliases = { ${aliases.join(', ')} }, feeTiers = ${luaList(FEE_TIER_ORDER)} }`;
}

/** A file's `local name = require('module')` line, which the bundle moves to the top of the chunk the file goes in. */
const REQUIRE = /^local (\w+) = require\((['"])([\w.]+)\2\)\n/gm;

/**
 * `parts` as one chunk, their requires first and each once, since the parts share the chunk's scope and a module is
 * the same table however often it's required. @internal
 */
export function chunk(parts: string[]): string {
	const requires = new Map<string, string>();

	const bodies = parts.map((part) =>
		part.replaceAll(REQUIRE, (_line, name: string, _quote, module: string) => {
			if ((requires.get(name) ?? module) !== module) throw new Error(`Two Lua files bind ${name} to different modules`);
			requires.set(name, module);

			return '';
		})
	);

	return [...[...requires].map(([name, module]) => `local ${name} = require('${module}')`), ...bodies].join('\n');
}

/** A lexicon's `ref` and `refs` fields, as JSON: a quoted ref, or an array of them. */
const REF_FIELD = /"refs?":("[^"]*"|\[[^\]]*\])/g;

/** The NSIDs of the other lexicons `doc` refers to, read from its JSON (refs to its own defs left out, local or not). */
function refsOf(doc: LexiconDoc): string[] {
	return [...JSON.stringify(doc.defs).matchAll(REF_FIELD)].flatMap(([, field]) =>
		[...field.matchAll(/"([^"]*)"/g)].flatMap(([, ref]) => {
			const nsid = ref.split('#')[0];

			return nsid === '' || nsid === doc.id ? [] : [nsid];
		})
	);
}

/**
 * `docs` and every lexicon they refer to, from `available`, in an order where each document's refs come before it,
 * since HappyView stores them in the bundle's order.
 */
function withRefs(docs: LexiconDoc[], available: Map<string, LexiconDoc>): LexiconDoc[] {
	const out = new Map<string, LexiconDoc>();

	const visit = (doc: LexiconDoc) => {
		if (out.has(doc.id)) return;

		for (const nsid of refsOf(doc)) {
			const ref = available.get(nsid);

			if (!ref) throw new Error(`${doc.id} refers to ${nsid}, which this app has no lexicon for`);
			visit(ref);
		}

		out.set(doc.id, doc);
	};

	for (const doc of docs) visit(doc);

	return [...out.values()];
}

/**
 * The lexicons and Lua scripts that make HappyView the directory for `rootDid`'s projects and what their managers and
 * fans publish. Every script outside `lua/lib` is picked up: a record script has HappyView index that collection, and a
 * query script has it serve that query. The bundle holds their lexicons and the ones those refer to, and no others.
 * Each script is one chunk that starts with the modules its files require, then defines the root DID. Queries then get
 * the genre and fee taxonomies (from the TypeScript, so they can't drift, written out as a Lua table so loading needs
 * no module: HappyView loads a script on upload with stand-ins for every module) and every `lua/lib` file before their
 * own code. Record scripts get the userinput.app space bug reports post to (`USERINPUT_SPACE`, nil without one) and
 * otherwise stand alone: HappyView stores a record when its script errors, so a gate shouldn't depend on anything else
 * loading, and one that reads SQL requires the library inside its pcall rather than at the top.
 */
export function happyViewBundle(rootDid: string, userinputSpace?: string): HappyViewBundle {
	const root = `local ROOT_DID = ${longString(rootDid)}`;
	const recordPrelude = [root, `local USERINPUT_SPACE = ${userinputSpace ? longString(userinputSpace) : 'nil'}`];
	const files = Object.entries(LUA_FILES).sort(([a], [b]) => a.localeCompare(b));
	const library = files.filter(([path]) => path.startsWith(LIBRARY)).map(([, body]) => body);
	const queryPrelude = [root, `local TAXONOMY = ${luaTaxonomy()}`, ...library];
	const available = new Map<string, LexiconDoc>([...REFERENCED_DOCS, ...LEXICON_DOCS].map((doc) => [doc.id, doc]));

	// `lua/<nsid path>.lua` handles the lexicon at that NSID: an index script for a record, the handler for a query.
	const handlers = files
		.filter(([path]) => !path.startsWith(LIBRARY))
		.map(([path, body]) => {
			const nsid = path.slice('/lua/'.length, -'.lua'.length).replaceAll('/', '.');
			const doc = available.get(nsid);
			const type = doc?.defs.main?.type;

			if (!doc || (type !== 'record' && type !== 'query'))
				throw new Error(`${path} matches no record or query lexicon this app has`);

			return { doc, id: `${type === 'record' ? 'record.index' : 'xrpc.query'}:${nsid}`, body };
		});

	return {
		lexicons: withRefs(
			handlers.map(({ doc }) => doc),
			available
		),
		scripts: handlers.map(({ id, body }) => ({
			id,
			body: chunk([...(id.startsWith('record.') ? recordPrelude : queryPrelude), body])
		}))
	};
}
