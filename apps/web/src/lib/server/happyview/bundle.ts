import type { LexiconDoc } from '@atproto/lexicon';
import { COLLECTION, NS } from '#lib/nsid.ts';
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

/**
 * The records HappyView indexes (projects and DJ sets), the defs they use, and the directory queries, in an order
 * where each document's refs come before it. HappyView needs nothing else.
 */
const LEXICONS = [
	...REFERENCED_DOCS,
	...LEXICON_DOCS.filter(
		(doc) =>
			[`${NS}.defs`, COLLECTION.project, `${NS}.dj.defs`, COLLECTION.djSet].includes(doc.id) ||
			doc.id.startsWith(`${NS}.directory.`)
	)
];

/** `text` as a Lua long string, with enough `=` in its brackets that nothing inside can close it early. */
function longString(text: string): string {
	let level = '';

	while (`${text}]`.includes(`]${level}]`)) level += '=';

	// Lua drops a newline right after the opening bracket, so one goes there to keep a leading newline in `text`.
	return `[${level}[\n${text}]${level}]`;
}

/** `lua/<nsid path>.lua` handles the lexicon at that NSID: an index script for a record, the handler for a query. */
function trigger(path: string): string {
	const nsid = path.slice('/lua/'.length, -'.lua'.length).replaceAll('/', '.');
	const type = LEXICONS.find((doc) => doc.id === nsid)?.defs.main?.type;

	if (type === 'record') return `record.index:${nsid}`;

	if (type === 'query') return `xrpc.query:${nsid}`;

	throw new Error(`${path} matches no record or query lexicon HappyView serves`);
}

/**
 * The lexicons and Lua scripts that make HappyView the directory for `rootDid`'s projects and their managers' sets. Each script is one chunk
 * that starts by defining the root DID. Queries then get the genre and fee taxonomies (from the TypeScript, so they
 * can't drift) and every `lua/lib` file before their own code. Record scripts stand alone: HappyView stores a
 * record when its script errors, so a gate shouldn't depend on anything else loading.
 */
export function happyViewBundle(rootDid: string): HappyViewBundle {
	const taxonomy = { families: GENRE_FAMILIES, aliases: Object.fromEntries(GENRE_ALIASES), feeTiers: FEE_TIER_ORDER };
	const root = `local ROOT_DID = ${longString(rootDid)}`;
	const files = Object.entries(LUA).sort(([a], [b]) => a.localeCompare(b));
	const library = files.filter(([path]) => path.startsWith(LIBRARY)).map(([, body]) => body);
	const queryPrelude = [root, `local TAXONOMY = json.decode(${longString(JSON.stringify(taxonomy))})`, ...library];

	return {
		lexicons: LEXICONS,
		scripts: files
			.filter(([path]) => !path.startsWith(LIBRARY))
			.map(([path, body]) => {
				const id = trigger(path);
				const prelude = id.startsWith('record.') ? [root] : queryPrelude;

				return { id, body: [...prelude, body].join('\n') };
			})
	};
}
