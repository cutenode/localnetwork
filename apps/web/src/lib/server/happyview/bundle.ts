import type { LexiconDoc } from '@atproto/lexicon';
import { GENRE_ALIASES, GENRE_FAMILIES } from '#lib/taxonomy/genres.ts';
import { FEE_TIER_ORDER } from '#lib/taxonomy/options.ts';
import { LEXICON_DOCS, REFERENCED_DOCS } from '../atproto/lexicons';

/** A HappyView script, keyed by its trigger, e.g. `xrpc.query:computer.localnetwork.directory.searchProjects`. */
export type HappyViewScript = { id: string; body: string };

/** Everything the app uploads to HappyView. */
export type HappyViewBundle = { lexicons: LexiconDoc[]; scripts: HappyViewScript[] };

/** Lua sources by path from the project root, e.g. `/lua/lib/directory.lua`. */
const LUA = import.meta.glob<string>('/lua/**/*.lua', { query: '?raw', import: 'default', eager: true });

const LIBRARY = '/lua/lib/';

/** `text` as a Lua long string, with enough `=` in its brackets that nothing inside can close it early. */
function longString(text: string): string {
	let level = '';

	while (`${text}]`.includes(`]${level}]`)) level += '=';

	// Lua drops a newline right after the opening bracket, so one goes there to keep a leading newline in `text`.
	return `[${level}[\n${text}]${level}]`;
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
 * Each script is one chunk that starts by defining the root DID. Queries then get the genre and
 * fee taxonomies (from the TypeScript, so they can't drift) and every `lua/lib` file before their own code. Record
 * scripts stand alone: HappyView stores a record when its script errors, so a gate shouldn't depend on anything else
 * loading.
 */
export function happyViewBundle(rootDid: string): HappyViewBundle {
	const taxonomy = { families: GENRE_FAMILIES, aliases: Object.fromEntries(GENRE_ALIASES), feeTiers: FEE_TIER_ORDER };
	const root = `local ROOT_DID = ${longString(rootDid)}`;
	const files = Object.entries(LUA).sort(([a], [b]) => a.localeCompare(b));
	const library = files.filter(([path]) => path.startsWith(LIBRARY)).map(([, body]) => body);
	const queryPrelude = [root, `local TAXONOMY = json.decode(${longString(JSON.stringify(taxonomy))})`, ...library];
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
			body: [...(id.startsWith('record.') ? [root] : queryPrelude), body].join('\n')
		}))
	};
}
