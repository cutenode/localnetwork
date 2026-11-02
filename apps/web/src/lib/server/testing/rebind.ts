import dns from 'node:dns';
import { once } from 'node:events';
import { syncBuiltinESMExports } from 'node:module';
import { type AddressInfo, createServer } from 'node:net';

/** Where a rebinding name leads: a server on loopback, and how many connections reached it. */
export type Loopback = { port: number; connections: () => number; close: () => Promise<void> };

/**
 * Makes `hosts` rebind to a loopback server that counts the connections it gets: DNS answers a public address to a
 * lookup made ahead of a request (with `dns/promises`, as a check would) and loopback to the one a connection makes.
 * `close` puts DNS back.
 */
export async function rebindToLoopback(hosts: string[]): Promise<Loopback> {
	let connections = 0;

	const server = createServer((socket) => {
		connections++;
		socket.destroy();
	});

	await once(server.listen(0, '127.0.0.1'), 'listening');
	// SAFETY: a server listening on a TCP port reports an AddressInfo.
	const { port } = server.address() as AddressInfo;
	const { lookup } = dns;
	const checkLookup = dns.promises.lookup;
	const rebinds = new Set(hosts);

	const checked = async (hostname: string, options: dns.LookupAllOptions): Promise<dns.LookupAddress[]> =>
		rebinds.has(hostname) ? [{ address: '93.184.216.34', family: 4 }] : checkLookup(hostname, options);

	const connected = (
		hostname: string,
		options: dns.LookupOptions,
		callback: (error: NodeJS.ErrnoException | null, address: string | dns.LookupAddress[], family?: number) => void
	) => {
		if (!rebinds.has(hostname)) return lookup(hostname, options, callback);

		if (options.all) callback(null, [{ address: '127.0.0.1', family: 4 }]);
		else callback(null, '127.0.0.1', 4);
	};

	// SAFETY: the check before connecting asks for every address, the one form `checked` takes.
	dns.promises.lookup = checked as typeof dns.promises.lookup;
	// SAFETY: a connection always calls lookup with (hostname, options, callback), the one form `connected` takes.
	dns.lookup = connected as typeof dns.lookup;
	syncBuiltinESMExports();

	return {
		port,
		connections: () => connections,
		close: async () => {
			dns.lookup = lookup;
			dns.promises.lookup = checkLookup;
			syncBuiltinESMExports();
			await new Promise((resolve) => server.close(resolve));
		}
	};
}
