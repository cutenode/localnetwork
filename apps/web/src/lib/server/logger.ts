import { AsyncLocalStorage } from 'node:async_hooks';
import { createLogger as startLine, type DrainContext, initLogger, type RequestLogger } from 'evlog';
import { z } from 'zod';

const REDACT = [
	'password',
	'*.password',
	'authorization',
	'*.authorization',
	'headers.authorization',
	'headers.cookie',
	'cookie',
	'accessJwt',
	'refreshJwt',
	'*.accessJwt',
	'*.refreshJwt',
	'token',
	'*.token',
	'credential',
	'*.credential',
	// Email addresses people share for their inbox.
	'email',
	'*.email',
	'address',
	'*.address'
];

const RANK = { trace: 0, debug: 1, info: 2, warn: 3, error: 4, fatal: 5, silent: 6 };

/** Past this many notes a line stops growing, and each further note gets a line of its own. */
const MAX_NOTES = 20;

type Fields = Parameters<RequestLogger['set']>[0];

type NoteLevel = 'debug' | 'info' | 'warn' | 'error';

type Note = (message: string, fields?: Fields) => void;

export type Logger = {
	debug: Note;
	info: Note;
	warn: Note;
	error: Note;
	/** Adds fields to the line of the request or job under way, not to any one note. */
	set(fields: Fields): void;
	/** A logger that adds `fields` to each of its notes. */
	child(fields: Fields): Logger;
	/**
	 * Runs `work` as a request: everything noted during it, here or in the services it calls, lands on one line, written
	 * when it answers. That line is `warn` for a 4xx and `error` for a 5xx. A `quiet` request is left out unless it noted
	 * something.
	 */
	request(fields: Fields, work: (log: Logger) => Promise<Response>, options?: { quiet?: boolean }): Promise<Response>;
	/** Runs `work` as a job, with one line as for a request, or none if it noted nothing: most runs have nothing to say. */
	job<T>(name: string, work: () => Promise<T>, fields?: Fields): Promise<T>;
};

/** One line in the making, for a request or job. Notes keep their own fields: `did` on a note isn't the viewer's. */
class Line {
	private readonly line: RequestLogger;
	private readonly messages = new Set<string>();
	private notes = 0;
	private level?: NoteLevel;
	private written = false;
	private failed = false;
	private readonly name: string;
	private readonly threshold: number;
	/** What the line opened with, which a note too late for it repeats. */
	readonly fields: Fields;

	// No parameter properties: scripts/db-backup.ts runs this file under plain Node, which only strips types.
	constructor(name: string, fields: Fields, threshold: number) {
		this.line = startLine(fields);
		this.name = name;
		this.fields = fields;
		this.threshold = threshold;
	}

	set(fields: Fields) {
		if (!this.written) this.line.set(fields);
	}

	private raise(level: NoteLevel) {
		if (!this.level || RANK[level] > RANK[this.level]) this.level = level;
	}

	settle(status: number) {
		this.set({ status });
		this.raise(status >= 500 ? 'error' : status >= 400 ? 'warn' : 'info');
	}

	/**
	 * False when the line has no room for it, or was already written. The first error goes on the line itself, where
	 * evlog prints it in full, and its note keeps the message.
	 */
	note(level: NoteLevel, message: string, { err, ...fields }: Fields) {
		if (this.written || this.notes === MAX_NOTES) return false;
		this.notes += 1;
		this.raise(level);
		this.messages.add(message);
		const hoisted = err instanceof Error && !this.failed;

		if (hoisted) {
			this.failed = true;
			this.line.error(err);
		}

		this.line.set({ notes: [{ ...fields, err: hoisted ? err.message : err, level, message }] });

		return true;
	}

	write({ quiet = false } = {}) {
		this.written = true;

		const level = this.level ?? 'info';

		if ((quiet && this.notes === 0) || RANK[level] < this.threshold) return;
		this.line.setLevel(level);
		// Railway searches `message`, so each note's stays findable, as it was with a line per note.
		this.line.emit({ message: [...this.messages].join('; ') || this.name });
	}
}

const underWay = new AsyncLocalStorage<Line>();

/** What SvelteKit's redirects and errors have in common: a `handle` that throws anything else answers 500. */
const thrownStatus = z.object({ status: z.number() });

function logger(threshold: number, bindings: Fields = {}, bound?: Line): Logger {
	const note = (level: NoteLevel): Note => {
		return (message, fields) => {
			if (RANK[level] < threshold) return;
			const line = bound ?? underWay.getStore();
			const entry = { ...bindings, ...fields };

			if (line?.note(level, message, entry)) return;
			const alone = new Line('note', line?.fields ?? {}, threshold);
			alone.note(level, message, entry);
			alone.write();
		};
	};

	return {
		debug: note('debug'),
		info: note('info'),
		warn: note('warn'),
		error: note('error'),
		set: (fields) => (bound ?? underWay.getStore())?.set(fields),
		child: (fields) => logger(threshold, { ...bindings, ...fields }, bound),

		async request(fields, work, options) {
			const line = new Line('request', fields, threshold);

			try {
				const response = await underWay.run(line, () => work(logger(threshold, bindings, line)));
				line.settle(response.status);

				return response;
			} catch (thrown) {
				line.settle(thrownStatus.safeParse(thrown).data?.status ?? 500);
				throw thrown;
			} finally {
				line.write(options);
			}
		},

		async job(name, work, fields = {}) {
			const line = new Line(name, { job: name, ...fields }, threshold);

			try {
				return await underWay.run(line, work);
			} finally {
				line.write({ quiet: true });
			}
		}
	};
}

/**
 * Railway files everything on stderr under `error`, whatever its `level` says, and evlog's console output sends `warn`
 * to stderr. So production lines all go to stdout and carry their level by name.
 */
const writeLine = ({ event }: DrainContext) => {
	process.stdout.write(`${JSON.stringify(event)}\n`);
};

/** Sets up evlog, which has one configuration per process, and gives a logger that writes at `level` and above. */
export function createLogger(options: { level: string; pretty: boolean }): Logger {
	initLogger({
		env: { service: 'localnetwork' },
		pretty: options.pretty,
		silent: !options.pretty,
		drain: options.pretty ? undefined : writeLine,
		redact: { paths: REDACT, replacement: '[redacted]' }
	});

	return logger(new Map(Object.entries(RANK)).get(options.level) ?? RANK.info);
}

/** A logger that discards everything, for tests and scripts that do not care. @internal */
export const silentLogger: Logger = logger(RANK.silent);
