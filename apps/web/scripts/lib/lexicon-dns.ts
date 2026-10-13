type TxtRecord = { zone_id: string; type: 'TXT'; name: string; content: string; ttl: 1 };

// Cloudflare answers writes with the record (a delete's id is optional); nothing here reads it.
type Written = { id?: string };

/** The parts of the `cloudflare` client this uses, so tests can pass a fake. */
export type DnsClient = {
	zones: { list(): AsyncIterable<{ id: string; name: string }> };
	dns: {
		records: {
			list(query: {
				zone_id: string;
				type: 'TXT';
				name: { exact: string };
			}): AsyncIterable<{ id: string; content?: string }>;
			create(record: TxtRecord): Promise<Written>;
			update(id: string, record: TxtRecord): Promise<Written>;
			delete(id: string, params: { zone_id: string }): Promise<Written>;
		};
	};
};

export type LexiconRecordChange = 'set' | 'created' | 'updated';

/**
 * Leaves each of `names` with exactly one `did=` TXT record, `expected`, in whichever of the
 * token's zones holds it. Other TXT values and other records at those names are never touched.
 */
export async function syncLexiconRecords(
	client: DnsClient,
	names: string[],
	expected: string
): Promise<Map<string, LexiconRecordChange>> {
	const zones = await Array.fromAsync(client.zones.list());
	const changes = new Map<string, LexiconRecordChange>();

	for (const name of names) {
		const zone = zones
			.filter((candidate) => name.endsWith(`.${candidate.name}`))
			.toSorted((a, b) => b.name.length - a.name.length)[0];

		if (!zone) throw new Error(`The Cloudflare token can't see the zone that holds ${name}.`);

		const records = (
			await Array.fromAsync(client.dns.records.list({ zone_id: zone.id, type: 'TXT', name: { exact: name } }))
		).filter((record) => unquote(record.content).startsWith('did='));

		const keep = records.find((record) => unquote(record.content) === expected) ?? records[0];
		// Cloudflare stores TXT content as quoted RFC 1035 character strings.
		const record: TxtRecord = { zone_id: zone.id, type: 'TXT', name, content: `"${expected}"`, ttl: 1 };

		if (!keep) {
			await client.dns.records.create(record);
			changes.set(name, 'created');
			continue;
		}

		// Extra `did=` values would leave the authority ambiguous, so only one survives.
		const extra = records.filter((other) => other !== keep);
		const isRight = unquote(keep.content) === expected;

		if (!isRight) await client.dns.records.update(keep.id, record);

		for (const other of extra) await client.dns.records.delete(other.id, { zone_id: zone.id });

		changes.set(name, isRight && extra.length === 0 ? 'set' : 'updated');
	}

	return changes;
}

function unquote(content = ''): string {
	return content.replace(/^"(.*)"$/, '$1');
}
