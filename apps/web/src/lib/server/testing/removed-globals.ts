import { type Node, parse } from 'luaparse';

/**
 * The globals HappyView 3 removed, in the order it names them (`REMOVED_GLOBALS` in its src/codemod/detect.rs). A
 * script now reaches HappyView through handle(input, ctx) and `require`.
 */
export const REMOVED_GLOBALS = [
	'db',
	'Record',
	'xrpc',
	'atproto',
	'linked_repos',
	'jobs',
	'job',
	'http',
	'log',
	'now',
	'TID',
	'json',
	'toarray',
	'input',
	'params',
	'event',
	'record',
	'action',
	'uri',
	'did',
	'rkey',
	'src',
	'val',
	'neg',
	'cts',
	'exp',
	'method',
	'collection',
	'caller_did',
	'delegate_did',
	'env',
	'space'
] as const;

/** The trigger kinds whose v2 globals differ, by a script's trigger id prefix. */
type Kind = 'query' | 'record' | 'unknown';

/** Names that were globals for some kinds only: a free `uri` was the record's URI in a record script. */
const SET_BY: Record<Kind, Set<string>> = {
	query: new Set(['params', 'method', 'collection']),
	record: new Set(['event', 'record', 'action', 'uri', 'did', 'collection', 'rkey']),
	unknown: new Set([
		'input',
		'record',
		'action',
		'uri',
		'did',
		'collection',
		'rkey',
		'src',
		'val',
		'neg',
		'cts',
		'exp',
		'event',
		'params',
		'method',
		'job'
	])
};

const KIND_DEPENDENT = SET_BY.unknown;

const CALLS = new Set(['CallExpression', 'StringCallExpression', 'TableCallExpression']);

/** luaparse marks each identifier it resolves with `isLocal` when asked to track scopes; its types leave that out. */
type Scoped = Node & { isLocal?: boolean };

function kindOf(trigger: string): Kind {
	if (trigger.startsWith('xrpc.query:')) return 'query';

	return trigger.startsWith('record.') ? 'record' : 'unknown';
}

/**
 * The removed globals a script still reads, in HappyView's order, the way HappyView 3 decides whether to refuse a
 * script on save (`needs_migration`): a free name only, so a local or parameter of the same name doesn't count, and a
 * global the script assigns anywhere is its own; `log` only where it's called; and a name that was only ever set for
 * some kinds of script only in those. A script luaparse can't read (Lua 5.4's attributes, say) is left to the load to
 * refuse.
 */
export function removedGlobals(trigger: string, body: string): string[] {
	const nodes: Scoped[] = [];

	try {
		parse(body, { scope: true, luaVersion: '5.3', onCreateNode: (node) => nodes.push(node) });
	} catch {
		return [];
	}

	const kind = kindOf(trigger);
	const assigned = new Set<string>();
	const called = new Set<Node>();
	const free: Scoped[] = [];

	for (const node of nodes) {
		if (node.type === 'AssignmentStatement')
			for (const variable of node.variables) if (variable.type === 'Identifier') assigned.add(variable.name);

		if (node.type === 'FunctionDeclaration' && !node.isLocal && node.identifier?.type === 'Identifier')
			assigned.add(node.identifier.name);

		if (CALLS.has(node.type) && 'base' in node) called.add(node.base);

		if (node.type === 'Identifier' && node.isLocal === false) free.push(node);
	}

	const read = new Set(
		free.flatMap((node) => {
			if (node.type !== 'Identifier' || assigned.has(node.name)) return [];

			if (node.name === 'log' && !called.has(node)) return [];

			return KIND_DEPENDENT.has(node.name) && !SET_BY[kind].has(node.name) ? [] : [node.name];
		})
	);

	return REMOVED_GLOBALS.filter((name) => read.has(name));
}
