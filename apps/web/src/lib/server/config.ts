import { existsSync } from 'node:fs';
import { z } from 'zod';

const booleanString = z.enum(['true', 'false', '1', '0', 'yes', 'no']).optional();

const intString = (fallback: number, min: number, max = Number.MAX_SAFE_INTEGER) =>
	z
		.string()
		.optional()
		.transform((value, ctx) => {
			if (value === undefined || value === '') return fallback;
			const parsed = Number(value);

			if (!Number.isSafeInteger(parsed) || parsed < min || parsed > max) {
				ctx.addIssue({ code: 'custom', message: `must be an integer from ${min} to ${max}` });

				return z.NEVER;
			}

			return parsed;
		});

const didList = z
	.string()
	.optional()
	.transform((value, ctx) => {
		const items = (value ?? '')
			.split(',')
			.map((item) => item.trim())
			.filter(Boolean);

		for (const item of items) {
			if (!item.startsWith('did:')) {
				ctx.addIssue({ code: 'custom', message: `"${item}" is not a DID` });

				return z.NEVER;
			}
		}

		return items;
	});

const absoluteUrl = z.url().transform((value) => value.replace(/\/+$/, ''));

/** A userinput.app space's at:// URI. Left empty, it reads as unset. */
const userinputSpace = z
	.string()
	.optional()
	.transform((value) => value || undefined)
	.pipe(
		z
			.string()
			.regex(
				/^at:\/\/did:[a-z]+:[\w.:%-]+\/app\.userinput\.space\/[\w.~:-]+$/,
				'must be at://<did>/app.userinput.space/<rkey>'
			)
			.optional()
	);

const envSchema = z.object({
	NODE_ENV: z.enum(['development', 'production', 'test']).default('development'),
	PUBLIC_URL: absoluteUrl.default('http://127.0.0.1:5173'),
	DATABASE_PATH: z.string().min(1).default('data/localnetwork.db'),
	LOG_LEVEL: z.enum(['fatal', 'error', 'warn', 'info', 'debug', 'trace', 'silent']).optional(),
	ROOT_IDENTIFIER: z.string().min(1, 'set ROOT_IDENTIFIER to the root account handle or DID'),
	ROOT_PASSWORD: z.string().min(1, 'set ROOT_PASSWORD to the root account password (not an app password)'),
	ROOT_SERVICE: absoluteUrl.optional(),
	ADMIN_DIDS: didList,
	PLC_URL: absoluteUrl.default('https://plc.directory'),
	HANDLE_RESOLVER_URL: absoluteUrl.optional(),
	HAPPYVIEW_URL: z.string('set HAPPYVIEW_URL to the HappyView instance that indexes projects').pipe(absoluteUrl),
	HAPPYVIEW_API_KEY: z.string().min(1).optional(),
	USERINPUT_SPACE: userinputSpace,
	SERVICE_DID: z.string().startsWith('did:').optional(),
	SYNC_POLL_INTERVAL_MS: intString(60_000, 1_000),
	SYNC_NOTIFY: booleanString,
	GEOCODER: z.enum(['open-meteo', 'nominatim', 'none']).default('open-meteo'),
	ALLOW_PRIVATE_NETWORK: booleanString,
	AUDIT_MAX_ATTEMPTS: intString(12, 1, 100),
	SESSION_DAYS: intString(30, 1, 365),
	EPK_VIEW_RETENTION_DAYS: intString(90, 1, 3650)
});

export type Config = {
	env: 'development' | 'production' | 'test';
	dev: boolean;
	publicUrl: string;
	databasePath: string;
	logLevel: string;
	root: { identifier: string; password: string; service?: string };
	adminDids: string[];
	plcUrl: string;
	handleResolverUrl?: string;
	/** The HappyView instance that indexes published projects. With an API key, the app uploads its lexicons and scripts. */
	happyView: { url: string; apiKey?: string };
	/** The userinput.app space signed-in people post bug reports to. Unset, every report goes to GitHub instead. */
	userinputSpace?: string;
	/** did:web identity of this app, used as the notification target for spaces. */
	serviceDid: string;
	/** Service id (DID + fragment) that space hosts deliver notifyWrite to. */
	syncService: string;
	syncPollIntervalMs: number;
	syncNotify: boolean;
	geocoder: 'open-meteo' | 'nominatim' | 'none';
	allowPrivateNetwork: boolean;
	auditMaxAttempts: number;
	sessionDays: number;
	/** How long receipts of press kit views are kept before they expire. */
	epkViewRetentionDays: number;
};

export const SYNC_SERVICE_FRAGMENT = 'localnetwork_syncer';

/**
 * did:web for this deployment. Loopback IPs become `localhost`, the only host
 * did:web resolvers fetch over plain HTTP (for local development).
 */
export function serviceDidFor(publicUrl: string): string {
	const url = new URL(publicUrl);
	const loopback = url.hostname === '127.0.0.1' || url.hostname === '[::1]';
	const host = loopback ? `localhost${url.port ? `:${url.port}` : ''}` : url.host;

	return `did:web:${encodeURIComponent(host)}`;
}

export function loadConfig(env: Record<string, string | undefined>): Config {
	const parsed = envSchema.safeParse(env);

	if (!parsed.success) {
		const problems = parsed.error.issues.map((issue) => `  ${issue.path.join('.')}: ${issue.message}`);
		throw new Error(`Invalid configuration:\n${problems.join('\n')}`);
	}

	const value = parsed.data;
	const dev = value.NODE_ENV !== 'production';
	const isHttps = value.PUBLIC_URL.startsWith('https:');
	const serviceDid = value.SERVICE_DID ?? serviceDidFor(value.PUBLIC_URL);

	const truthy = (input: string | undefined, fallback: boolean) =>
		input === undefined ? fallback : ['true', '1', 'yes'].includes(input);

	return {
		env: value.NODE_ENV,
		dev,
		publicUrl: value.PUBLIC_URL,
		databasePath: value.DATABASE_PATH,
		logLevel: value.LOG_LEVEL ?? (value.NODE_ENV === 'test' ? 'silent' : dev ? 'debug' : 'info'),
		root: {
			identifier: value.ROOT_IDENTIFIER,
			password: value.ROOT_PASSWORD,
			service: value.ROOT_SERVICE
		},
		adminDids: value.ADMIN_DIDS,
		plcUrl: value.PLC_URL,
		handleResolverUrl: value.HANDLE_RESOLVER_URL,
		happyView: { url: value.HAPPYVIEW_URL, apiKey: value.HAPPYVIEW_API_KEY },
		userinputSpace: value.USERINPUT_SPACE,
		serviceDid,
		syncService: `${serviceDid}#${SYNC_SERVICE_FRAGMENT}`,
		syncPollIntervalMs: value.SYNC_POLL_INTERVAL_MS,
		syncNotify: truthy(value.SYNC_NOTIFY, isHttps || dev),
		geocoder: value.GEOCODER,
		allowPrivateNetwork: truthy(value.ALLOW_PRIVATE_NETWORK, dev),
		auditMaxAttempts: value.AUDIT_MAX_ATTEMPTS,
		sessionDays: value.SESSION_DAYS,
		epkViewRetentionDays: value.EPK_VIEW_RETENTION_DAYS
	};
}

let cached: Config | undefined;

/**
 * Loads `.env.local` then `.env` (without overriding real environment
 * variables) and parses the result once per process.
 */
export function getConfig(): Config {
	if (cached) return cached;

	if (process.env.NODE_ENV !== 'production') {
		for (const file of ['.env.local', '.env']) {
			if (existsSync(file)) process.loadEnvFile(file);
		}
	}

	cached = loadConfig(process.env);

	return cached;
}
