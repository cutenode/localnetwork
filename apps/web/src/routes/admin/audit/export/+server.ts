import { requireStaff } from '#lib/server/http.ts';
import type { RequestHandler } from './$types';

/** Streams the full audit log as JSON Lines, oldest first. */
export const GET: RequestHandler = ({ locals, url }) => {
	const viewer = requireStaff(locals, url);
	const { app } = locals;
	const encoder = new TextEncoder();
	let lastSeq = 0;

	const stream = new ReadableStream<Uint8Array>({
		async pull(controller) {
			const batch = await app.db
				.selectFrom('auditEntry')
				.selectAll()
				.where('seq', '>', lastSeq)
				.orderBy('seq')
				.limit(500)
				.execute();

			if (batch.length === 0) {
				controller.close();

				return;
			}

			for (const entry of batch) controller.enqueue(encoder.encode(`${JSON.stringify(entry)}\n`));
			lastSeq = batch.at(-1)!.seq;
		}
	});

	locals.log.info({ did: viewer.did }, 'audit log exported');

	return new Response(stream, {
		headers: {
			'content-type': 'application/x-ndjson; charset=utf-8',
			'content-disposition': `attachment; filename="localnetwork-audit-${new Date().toISOString().slice(0, 10)}.jsonl"`,
			'cache-control': 'no-store'
		}
	});
};
