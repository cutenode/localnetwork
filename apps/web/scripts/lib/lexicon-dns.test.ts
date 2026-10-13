import { describe, expect, it } from 'vitest';
import { type DnsClient, syncLexiconRecords } from './lexicon-dns.ts';

type StoredRecord = { id: string; zone_id: string; type: string; name: string; content: string };

const ROOT = 'did=did:plc:root';

/** An in-memory Cloudflare account holding `records`, which it changes in place. */
function fakeCloudflare(zones: { id: string; name: string }[], records: StoredRecord[]): DnsClient {
	let nextId = records.length;

	// Cloudflare answers 404 for an unknown record id.
	const indexOf = (id: string) => {
		const index = records.findIndex((record) => record.id === id);

		if (index === -1) throw new Error(`No record ${id}`);

		return index;
	};

	return {
		zones: {
			list: async function* () {
				yield* zones;
			}
		},
		dns: {
			records: {
				list: async function* (query) {
					yield* records.filter(
						(record) =>
							record.zone_id === query.zone_id && record.type === query.type && record.name === query.name.exact
					);
				},
				create: async (record) => {
					const id = `new-${nextId++}`;

					records.push({ ...record, id });

					return { id };
				},
				update: async (id, record) => {
					records[indexOf(id)] = { ...record, id };

					return { id };
				},
				delete: async (id) => {
					records.splice(indexOf(id), 1);

					return { id };
				}
			}
		}
	};
}

const txt = (id: string, name: string, content: string): StoredRecord => ({
	id,
	zone_id: 'zone',
	type: 'TXT',
	name,
	content
});

describe('syncLexiconRecords', () => {
	it('creates missing records, fixes wrong ones, and leaves everything else alone', async () => {
		const unrelated = [
			txt('spf', '_lexicon.dj.localnetwork.computer', '"v=spf1 -all"'),
			txt('other-name', '_atproto.localnetwork.computer', '"did=did:plc:someone"'),
			{ ...txt('a', '_lexicon.localnetwork.computer', '1.2.3.4'), type: 'A' }
		];

		const records = [
			...unrelated,
			txt('right', '_lexicon.localnetwork.computer', `"${ROOT}"`),
			txt('wrong', '_lexicon.space.localnetwork.computer', '"did=did:plc:old"'),
			txt('dup-right', '_lexicon.staff.localnetwork.computer', `"${ROOT}"`),
			txt('dup-wrong', '_lexicon.staff.localnetwork.computer', '"did=did:plc:old"')
		];

		const names = [
			'_lexicon.localnetwork.computer',
			'_lexicon.space.localnetwork.computer',
			'_lexicon.staff.localnetwork.computer',
			'_lexicon.dj.localnetwork.computer'
		];

		const changes = await syncLexiconRecords(
			fakeCloudflare([{ id: 'zone', name: 'localnetwork.computer' }], records),
			names,
			ROOT
		);

		expect(Object.fromEntries(changes)).toEqual({
			'_lexicon.localnetwork.computer': 'set',
			'_lexicon.space.localnetwork.computer': 'updated',
			'_lexicon.staff.localnetwork.computer': 'updated',
			'_lexicon.dj.localnetwork.computer': 'created'
		});
		expect(records).toEqual(expect.arrayContaining(unrelated));
		expect(records.filter((record) => record.content.startsWith('"did=') && names.includes(record.name))).toEqual(
			names.map((name) => expect.objectContaining({ name, type: 'TXT', content: `"${ROOT}"` }))
		);
	});

	it('changes nothing on a second run', async () => {
		const records: StoredRecord[] = [];
		const client = fakeCloudflare([{ id: 'zone', name: 'localnetwork.computer' }], records);
		const names = ['_lexicon.localnetwork.computer', '_lexicon.dj.localnetwork.computer'];

		await syncLexiconRecords(client, names, ROOT);
		const written = structuredClone(records);

		expect([...(await syncLexiconRecords(client, names, ROOT)).values()]).toEqual(['set', 'set']);
		expect(records).toEqual(written);
	});

	it('accepts unquoted content as right', async () => {
		const records = [txt('bare', '_lexicon.localnetwork.computer', ROOT)];

		const changes = await syncLexiconRecords(
			fakeCloudflare([{ id: 'zone', name: 'localnetwork.computer' }], records),
			['_lexicon.localnetwork.computer'],
			ROOT
		);

		expect(changes.get('_lexicon.localnetwork.computer')).toBe('set');
		expect(records).toEqual([txt('bare', '_lexicon.localnetwork.computer', ROOT)]);
	});

	it('writes to the most specific zone the token can see', async () => {
		const records: StoredRecord[] = [];

		const zones = [
			{ id: 'apex', name: 'localnetwork.computer' },
			{ id: 'dj', name: 'dj.localnetwork.computer' }
		];

		await syncLexiconRecords(fakeCloudflare(zones, records), ['_lexicon.dj.localnetwork.computer'], ROOT);

		expect(records).toEqual([expect.objectContaining({ zone_id: 'dj', name: '_lexicon.dj.localnetwork.computer' })]);
	});

	it('fails without writing when no zone holds a name', async () => {
		const records: StoredRecord[] = [];

		await expect(
			syncLexiconRecords(
				fakeCloudflare([{ id: 'lookalike', name: 'network.computer' }], records),
				['_lexicon.localnetwork.computer'],
				ROOT
			)
		).rejects.toThrow("can't see the zone that holds _lexicon.localnetwork.computer");
		expect(records).toEqual([]);
	});
});
