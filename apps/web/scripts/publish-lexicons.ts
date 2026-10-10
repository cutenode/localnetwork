/**
 * Publishes every tool's computer.localnetwork.* lexicons (the repo's
 * lexicons/computer/, so locallibrary's too) as com.atproto.lexicon.schema
 * records in the root account's repo, then checks the `_lexicon` DNS TXT
 * records that make them resolvable and exits non-zero while any are missing
 * or point at another DID. PDSes must resolve the space type declarations
 * during OAuth, so run this before anyone signs in.
 *
 *   ROOT_IDENTIFIER=… ROOT_PASSWORD=… pnpm lexicons:publish
 *
 * Finds the root account like the app does (PLC_URL, HANDLE_RESOLVER_URL, and
 * ROOT_SERVICE to skip the PDS lookup). Reads `.env` but not `.env.local`: that file
 * points at the temporary dev network, and `pnpm dev:network` publishes the
 * lexicons there itself.
 */
import { existsSync, readdirSync, readFileSync } from 'node:fs';
import { resolveTxt } from 'node:dns/promises';
import { join } from 'node:path';
import { AtpAgent } from '@atproto/api';
import { parseLexiconDoc } from '@atproto/lexicon';
import { canonicalJson } from '../src/lib/server/audit/chain.ts';
import { IdentityResolver } from '../src/lib/server/atproto/identity.ts';
import { AgentRootClient } from '../src/lib/server/atproto/repo-client.ts';
import { silentLogger } from '../src/lib/server/logger.ts';

if (existsSync('.env')) process.loadEnvFile('.env');

// Ours only: lexicons/community/ holds copies of other authorities' lexicons, which they publish.
const ours = join(import.meta.dirname, '../../../lexicons/computer');

const docs = readdirSync(ours, { recursive: true, encoding: 'utf8' })
	.filter((file) => file.endsWith('.json'))
	.map((file) => parseLexiconDoc(JSON.parse(readFileSync(join(ours, file), 'utf8'))));

const identifier = process.env.ROOT_IDENTIFIER;

const password = process.env.ROOT_PASSWORD;

if (!identifier || !password) {
	console.error('Set ROOT_IDENTIFIER and ROOT_PASSWORD (see .env.example).');
	process.exit(1);
}

const identity = new IdentityResolver({
	plcUrl: process.env.PLC_URL ?? 'https://plc.directory',
	handleResolverUrl: process.env.HANDLE_RESOLVER_URL,
	allowPrivateNetwork: false,
	logger: silentLogger
});

const did = await identity.resolveIdentifier(identifier);

const service = process.env.ROOT_SERVICE ?? (await identity.resolve(did)).pds;

const agent = new AtpAgent({ service });

await agent.login({ identifier: did, password });

const root = new AgentRootClient(did, agent);

for (const doc of docs) {
	const record = { $type: 'com.atproto.lexicon.schema', ...JSON.parse(JSON.stringify(doc)) };

	const existing = await root.getRecord({ collection: 'com.atproto.lexicon.schema', rkey: doc.id });

	if (existing && canonicalJson(existing.value) === canonicalJson(record)) {
		console.log(`unchanged  ${doc.id}`);
		continue;
	}

	await root.putRecord({ collection: 'com.atproto.lexicon.schema', rkey: doc.id, record });
	console.log(`published  ${doc.id}`);
}

const authorities = new Set(docs.map((doc) => doc.id.split('.').slice(0, -1).toReversed().join('.')));

const expected = `did=${did}`;

console.log(`\nDNS: each name needs TXT "${expected}" so PDSes can resolve the lexicons:\n`);

let unresolved = 0;

for (const authority of [...authorities].toSorted((a, b) => a.localeCompare(b))) {
	const name = `_lexicon.${authority}`;
	const values = await lexiconRecords(name);

	if (values.length > 0 && values.every((value) => value === expected)) {
		console.log(`  set      ${name}`);
		continue;
	}

	unresolved++;
	console.log(values.length === 0 ? `  missing  ${name}` : `  wrong    ${name}  (${values.join(', ')})`);
}

if (unresolved > 0) {
	console.error(`\n${unresolved} of ${authorities.size} _lexicon records are missing or wrong.`);
	process.exitCode = 1;
}

/** The `did=` TXT values at `name`; none when the name does not exist. */
async function lexiconRecords(name: string): Promise<string[]> {
	try {
		return (await resolveTxt(name)).map((chunks) => chunks.join('')).filter((value) => value.startsWith('did='));
	} catch (error) {
		// SAFETY: node:dns rejects with an Error carrying a DNS error code.
		const { code } = error as NodeJS.ErrnoException;

		if (code === 'ENOTFOUND' || code === 'ENODATA') return [];

		throw error;
	}
}
