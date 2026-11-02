import { createRequire } from 'node:module';
import pino, { type DestinationStream, type Logger } from 'pino';
import type PinoPretty from 'pino-pretty';

export type { Logger };

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

/**
 * Pretty output in development. pino-pretty is a dev dependency and is used as
 * an in-process stream (pino's worker transports cannot resolve modules inside
 * Vite's SSR runner); production logs stay as JSON lines.
 */
function prettyStream(): DestinationStream | undefined {
	try {
		// SAFETY: pino-pretty sets module.exports to the factory its bundled types declare (`export = PinoPretty`).
		const pretty = createRequire(import.meta.url)('pino-pretty') as typeof PinoPretty;

		return pretty({ colorize: true, translateTime: 'HH:MM:ss.l', ignore: 'pid,hostname', sync: true });
	} catch {
		return undefined;
	}
}

export function createLogger(options: { level: string; pretty: boolean; name?: string }): Logger {
	const settings = {
		name: options.name ?? 'localnetwork',
		level: options.level,
		redact: { paths: REDACT, censor: '[redacted]' },
		serializers: { err: pino.stdSerializers.err, error: pino.stdSerializers.err }
	};

	const stream = options.pretty ? prettyStream() : undefined;

	return stream ? pino(settings, stream) : pino(settings);
}

/** A logger that discards everything, for tests and scripts that do not care. */
export const silentLogger: Logger = pino({ level: 'silent' });
