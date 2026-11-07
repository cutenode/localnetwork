import { describe, expect, it, vi } from 'vitest';
import { createLogger, silentLogger } from './logger';
import { writtenLog } from './testing/written-log';

const SECRET = 's3cret-value';

describe('a line', () => {
	it('names its level and message, so failures do not show as info in the Railway logs', () => {
		const { logger, lines } = writtenLog('info');

		logger.info('listening');
		logger.warn('space sync failed');
		logger.error('bootstrap failed');

		expect(lines.map(({ level, message, service }) => [level, message, service])).toEqual([
			['info', 'listening', 'localnetwork'],
			['warn', 'space sync failed', 'localnetwork'],
			['error', 'bootstrap failed', 'localnetwork']
		]);
	});

	it('goes to stdout whatever its level, since Railway calls everything on stderr an error', () => {
		const stderr = vi.spyOn(process.stderr, 'write');
		const warn = vi.spyOn(console, 'warn');
		const error = vi.spyOn(console, 'error');
		const { logger, lines } = writtenLog('info');

		logger.warn('space sync failed');
		logger.error('bootstrap failed');

		expect(lines).toHaveLength(2);
		expect([stderr, warn, error].map((written) => written.mock.calls)).toEqual([[], [], []]);
	});

	it('leaves out notes below its level', () => {
		const { logger, lines } = writtenLog('warn');

		logger.debug('resolving');
		logger.info('listening');
		logger.warn('space sync failed');

		expect(lines.map((line) => line.message)).toEqual(['space sync failed']);
	});

	it('survives a field that holds itself', () => {
		const { logger, notes } = writtenLog('info');
		const cycle = { self: {} };
		cycle.self = cycle;

		logger.warn('odd fields', { cycle });

		expect(notes()).toEqual([{ level: 'warn', message: 'odd fields', cycle: { self: '[Circular]' } }]);
	});

	it('treats a level it does not know as info', () => {
		const { logger, lines } = writtenLog('loud');

		logger.debug('resolving');
		logger.info('listening');

		expect(lines.map((line) => line.message)).toEqual(['listening']);
	});

	it('writes nothing from the silent logger', async () => {
		const { lines } = writtenLog();

		silentLogger.error('dropped');
		await silentLogger.job('quiet', async () => silentLogger.error('dropped'));
		await silentLogger.request({}, async (log) => {
			log.error('dropped');

			return new Response('ok');
		});

		expect(lines).toEqual([]);
	});
});

describe('redaction', () => {
	// Pino's paths, which matched at the top level and one level down.
	it.each([
		'password',
		'authorization',
		'cookie',
		'accessJwt',
		'refreshJwt',
		'token',
		'credential',
		'email',
		'address'
	])('keeps %s out of the output, wherever it sits', (key) => {
		const { logger, lines } = writtenLog();

		logger.warn('held', { [key]: SECRET, session: { [key]: SECRET }, deep: { er: { [key]: SECRET } } });

		const [note] = lines[0].notes!;

		expect(note).toMatchObject({
			[key]: '[redacted]',
			session: { [key]: '[redacted]' },
			deep: { er: { [key]: '[redacted]' } }
		});
		expect(JSON.stringify(lines)).not.toContain(SECRET);
	});

	it('keeps the headers pino named out too, on a note and on the line', async () => {
		const { logger, lines } = writtenLog();

		await logger.job('with headers', async () => {
			logger.set({ headers: { authorization: `Bearer ${SECRET}`, cookie: `session=${SECRET}`, accept: 'text/html' } });
			logger.warn('held', { headers: { authorization: `Bearer ${SECRET}`, cookie: `session=${SECRET}` } });
		});

		expect(lines[0].headers).toEqual({ authorization: '[redacted]', cookie: '[redacted]', accept: 'text/html' });
		expect(JSON.stringify(lines)).not.toContain(SECRET);
	});

	it('scrubs what an error brings along, and leaves other fields be', () => {
		const { logger, lines } = writtenLog();
		const error = (message: string) => Object.assign(new Error(message), { data: { password: SECRET } });

		logger.warn('could not sign in', { err: error('refused'), did: 'did:plc:dj' });

		expect(lines[0]).toMatchObject({
			error: { name: 'Error', message: 'refused', data: { password: '[redacted]' } },
			notes: [{ did: 'did:plc:dj', err: 'refused' }]
		});
		expect(JSON.stringify(lines)).not.toContain(SECRET);
	});

	it('still redacts in development, where the console is pretty', () => {
		const printed: string[] = [];
		const logger = createLogger({ level: 'info', pretty: true });
		const log = vi.spyOn(console, 'log').mockImplementation((...line) => void printed.push(line.join(' ')));

		const stdout = vi.spyOn(process.stdout, 'write').mockImplementation((line) => {
			printed.push(String(line));

			return true;
		});

		logger.warn('held', { password: SECRET });
		log.mockRestore();
		stdout.mockRestore();

		expect(printed.join('\n')).toContain('held');
		expect(printed.join('\n')).not.toContain(SECRET);
	});
});

describe('a request', () => {
	const answer = (status: number) => new Response(null, { status });

	it('is one line, whatever services noted along the way, each note with its own fields', async () => {
		const { logger, lines } = writtenLog('info');
		const service = logger.child({ component: 'spaces' });

		await logger.request({ requestId: 'req-1', method: 'GET', path: '/dashboard' }, async (log) => {
			log.set({ did: 'did:plc:viewer' });
			service.warn('could not read a space', { did: 'did:plc:someone', status: 404 });
			await Promise.resolve();
			service.info('wrote a listing');

			return answer(200);
		});

		expect(lines).toHaveLength(1);
		expect(lines[0]).toMatchObject({
			level: 'warn',
			message: 'could not read a space; wrote a listing',
			requestId: 'req-1',
			method: 'GET',
			path: '/dashboard',
			status: 200,
			did: 'did:plc:viewer',
			durationMs: expect.any(Number)
		});
		expect(lines[0].notes).toEqual([
			{
				level: 'warn',
				message: 'could not read a space',
				component: 'spaces',
				did: 'did:plc:someone',
				status: 404
			},
			{ level: 'info', message: 'wrote a listing', component: 'spaces' }
		]);
	});

	it('takes its level from its status, unless a note says worse', async () => {
		const { logger, lines } = writtenLog('info');

		await logger.request({ path: '/fine' }, async () => answer(200));
		await logger.request({ path: '/gone' }, async () => answer(404));
		await logger.request({ path: '/broken' }, async () => answer(500));
		await logger.request({ path: '/worse' }, async (log) => {
			log.error('bad data');

			return answer(404);
		});

		expect(lines.map(({ level, path, message }) => [level, path, message])).toEqual([
			['info', '/fine', 'request'],
			['warn', '/gone', 'request'],
			['error', '/broken', 'request'],
			['error', '/worse', 'bad data']
		]);
	});

	it('writes its line when it throws, with the status of a redirect or an error', async () => {
		const { logger, lines } = writtenLog('info');

		const thrown = (value: Error | { status: number }) =>
			logger
				.request({}, async () => {
					throw value;
				})
				.catch((error) => error);

		const redirect = { status: 303, location: '/login' };

		expect(await thrown(redirect)).toBe(redirect);
		await thrown({ status: 403 });
		await thrown(new Error('boom'));

		expect(lines.map(({ level, status }) => [level, status])).toEqual([
			['info', 303],
			['warn', 403],
			['error', 500]
		]);
	});

	it('keeps notes below the level off a line that is written anyway', async () => {
		const { logger, lines } = writtenLog('info');

		await logger.request({}, async (log) => {
			log.debug('resolved a handle');

			return answer(200);
		});

		expect(lines.map(({ message, notes }) => [message, notes])).toEqual([['request', undefined]]);
	});

	it('leaves out a quiet request unless it noted something', async () => {
		const { logger, lines } = writtenLog('info');

		await logger.request({ path: '/healthz' }, async () => answer(200), { quiet: true });
		await logger.request(
			{ path: '/_app/start.js' },
			async (log) => {
				log.warn('stale asset');

				return answer(200);
			},
			{ quiet: true }
		);

		expect(lines.map((line) => line.path)).toEqual(['/_app/start.js']);
	});

	it('ignores fields set after its line, without complaint', async () => {
		const warn = vi.spyOn(console, 'warn');
		const { logger, lines } = writtenLog('info');
		let late = () => {};

		await logger.request({}, async (log) => {
			late = () => log.set({ card: { bytes: 12 } });

			return answer(200);
		});

		late();

		expect(lines[0].card).toBeUndefined();
		expect(warn.mock.calls).toEqual([]);
	});

	it('still writes a note that comes after its line, tied back to the request', async () => {
		const { logger, lines } = writtenLog('info');
		let late = () => {};

		await logger.request({ requestId: 'req-1', path: '/dashboard/data' }, async (log) => {
			late = () => log.warn('could not list the records held for someone');

			return answer(200);
		});

		late();

		expect(lines.map(({ level, message, requestId, path }) => [level, message, requestId, path])).toEqual([
			['info', 'request', 'req-1', '/dashboard/data'],
			['warn', 'could not list the records held for someone', 'req-1', '/dashboard/data']
		]);
	});
});

describe('a job', () => {
	it('is silent when it notes nothing, however many times it runs', async () => {
		const { logger, lines } = writtenLog();

		logger.set({ outside: true });
		await logger.job('notification-delivery', async () => logger.set({ sent: 0 }));

		expect(lines).toEqual([]);
	});

	it('gathers its notes on one line under its name, with what it set', async () => {
		const { logger, lines } = writtenLog('info');
		const sync = logger.child({ component: 'sync' });

		const result = await logger.job(
			'space-sync',
			async () => {
				logger.set({ changed: 3 });
				sync.warn('space sync failed', { err: new Error('host unreachable') });

				return 'done';
			},
			{ space: 'at://did:plc:root/space/gig' }
		);

		expect(result).toBe('done');
		expect(lines).toHaveLength(1);
		expect(lines[0]).toMatchObject({
			level: 'warn',
			message: 'space sync failed',
			job: 'space-sync',
			space: 'at://did:plc:root/space/gig',
			changed: 3,
			error: { name: 'Error', message: 'host unreachable', stack: expect.stringContaining('logger.test') },
			notes: [{ component: 'sync', err: 'host unreachable' }]
		});
	});

	it('puts its first error on the line and keeps the others on their notes', async () => {
		const { logger, lines } = writtenLog('info');

		await logger.job('reconcile', async () => {
			logger.warn('first', { err: new Error('one') });
			logger.warn('second', { err: new Error('two') });
			logger.warn('not an error', { err: 'connection reset' });
		});

		expect(lines[0].error).toMatchObject({ message: 'one' });
		expect(lines[0].notes).toMatchObject([
			{ message: 'first', err: 'one' },
			{ message: 'second', err: { message: 'two', stack: expect.any(String) } },
			{ message: 'not an error', err: 'connection reset' }
		]);
	});

	it('keeps its own line inside a request, and passes on what it throws', async () => {
		const { logger, lines } = writtenLog('info');

		await logger.request({ path: '/xrpc/notifyWrite' }, async (log) => {
			await logger
				.job('space-sync', async () => {
					logger.warn('space sync failed');
					throw new Error('host unreachable');
				})
				.catch(() => log.warn('notified sync failed'));

			return new Response(null);
		});

		expect(lines.map(({ message, job, path }) => [message, job, path])).toEqual([
			['space sync failed', 'space-sync', undefined],
			['notified sync failed', undefined, '/xrpc/notifyWrite']
		]);
	});

	it('runs past its room for notes without losing any', async () => {
		const { logger, lines, notes } = writtenLog('info');

		await logger.job('reindex', async () => {
			for (const index of Array.from({ length: 22 }, (_, n) => n)) logger.warn('skipping', { index });
		});

		expect(lines.map((line) => line.notes?.length)).toEqual([1, 1, 20]);
		expect(lines.every((line) => line.job === 'reindex')).toBe(true);
		expect(
			notes()
				.map((note) => note.index)
				.toSorted((a, b) => Number(a) - Number(b))
		).toEqual(Array.from({ length: 22 }, (_, n) => n));
	});
});
