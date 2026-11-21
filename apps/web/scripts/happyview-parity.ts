/**
 * Checks that another HappyView answers the directory the way the current one does, before HAPPYVIEW_URL moves to it.
 * First it brings the new one in step the way the app does on boot (uploads this app's lexicons and Lua scripts, has it
 * re-read the root's projects and the records the app re-reads, and waits for those), unless `--compare-only`. Then it
 * asks both instances every computer.localnetwork.directory.* query about the root and the projects, DJs, shows, sets,
 * artists and records the old one knows, and prints each answer that differs. Exits non-zero when any does.
 *
 *   ROOT_IDENTIFIER=localnetwork.computer HAPPYVIEW_API_KEY=… pnpm happyview:parity <old url> <new url> [--compare-only]
 *
 * HAPPYVIEW_API_KEY is the new instance's key, with the permissions HAPPYVIEW_PERMISSIONS lists; `--compare-only`
 * needs none. With HAPPYVIEW_CLIENT_KEY and HAPPYVIEW_CLIENT_SECRET, the new instance is asked as the website's API
 * client, for an instance that refuses anonymous XRPC. USERINPUT_SPACE goes into the record scripts as in the app. Finds
 * the root account like the app does (PLC_URL, HANDLE_RESOLVER_URL).
 */
import { parseArgs } from 'node:util';
import { IdentityResolver } from '../src/lib/server/atproto/identity.ts';
import { happyViewBundle } from '../src/lib/server/happyview/bundle.ts';
import { HappyViewClient } from '../src/lib/server/happyview/client.ts';
import { silentLogger } from '../src/lib/server/logger.ts';
import { compare, parityCases, syncLikeTheApp } from './lib/happyview-parity.ts';

/** How long each re-read gets to finish before the comparison goes ahead without it. */
const REREAD_WAIT_MS = 5 * 60_000;

/** How much of each difference is printed. */
const DIFF_LINES = 40;

const { values: options, positionals } = parseArgs({
	allowPositionals: true,
	options: { 'compare-only': { type: 'boolean', default: false } }
});

const [oldUrl, newUrl] = positionals;

const identifier = process.env.ROOT_IDENTIFIER;

const apiKey = process.env.HAPPYVIEW_API_KEY;

const compareOnly = options['compare-only'];

if (!oldUrl || !newUrl || !identifier || (!compareOnly && !apiKey)) {
	console.error(
		'Usage: ROOT_IDENTIFIER=… HAPPYVIEW_API_KEY=… pnpm happyview:parity <old url> <new url> [--compare-only]'
	);
	process.exit(1);
}

const identity = new IdentityResolver({
	plcUrl: process.env.PLC_URL ?? 'https://plc.directory',
	handleResolverUrl: process.env.HANDLE_RESOLVER_URL,
	allowPrivateNetwork: false,
	logger: silentLogger
});

const rootDid = await identity.resolveIdentifier(identifier);

const userinputSpace = process.env.USERINPUT_SPACE || undefined;

const bundle = happyViewBundle(rootDid, userinputSpace);

const old = new HappyViewClient({ url: oldUrl.replace(/\/$/, '') });

const clientKey = process.env.HAPPYVIEW_CLIENT_KEY;

const clientSecret = process.env.HAPPYVIEW_CLIENT_SECRET;

const next = new HappyViewClient({
	url: newUrl.replace(/\/$/, ''),
	apiKey,
	client: clientKey && clientSecret ? { key: clientKey, secret: clientSecret } : undefined
});

if (!compareOnly) {
	console.log(`Syncing ${next.url} as the app would, for ${rootDid}`);
	const statuses = await syncLikeTheApp(next, bundle, { rootDid, userinputSpace, waitMs: REREAD_WAIT_MS });

	for (const [status, count] of Object.entries(statuses)) console.log(`  ${count} re-reads ${status}`);
}

const collections = bundle.scripts.flatMap(({ id }) => (id.startsWith('record.index:') ? [id.split(':')[1]] : []));

const cases = await parityCases(old, rootDid, collections);

console.log(`Comparing ${cases.length} queries between ${old.url} and ${next.url}`);

const mismatches = await compare(cases, old, next);

for (const { name, params, diff } of mismatches) {
	console.log(`\n${name} ${JSON.stringify(params)}`);
	console.log(diff.split('\n').slice(0, DIFF_LINES).join('\n'));
}

console.log(`\n${cases.length - mismatches.length} of ${cases.length} queries answered the same`);

process.exitCode = mismatches.length > 0 ? 1 : 0;
