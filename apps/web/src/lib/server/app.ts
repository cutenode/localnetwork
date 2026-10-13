import type { Viewer } from '#lib/types.ts';
import { SpaceCredentialManager } from './atproto/credentials';
import { IdentityResolver } from './atproto/identity';
import { grantsAll, OAuthService } from './atproto/oauth';
import { PublicRepo } from './atproto/public-repo';
import type { RepoClient } from './atproto/repo-client';
import { RootAccount } from './atproto/root';
import { rootSpaces } from './atproto/spaces';
import { AuditLog } from './audit/log';
import { WebSessions } from './auth/session';
import { ensureRootSpaces } from './bootstrap';
import { type Config, getConfig } from './config';
import type { Core } from './core';
import { type Db, nowIso, openDatabase } from './db';
import { Geocoder } from './geocode';
import { happyViewBundle } from './happyview/bundle';
import { HappyViewClient } from './happyview/client';
import { syncHappyView } from './happyview/sync';
import { createLogger, type Logger } from './logger';
import { RateLimiter } from './rate-limit';
import { AccountService } from './services/accounts';
import { BookingService } from './services/bookings';
import { PlannerService } from './services/planner';
import { ProjectAdmin } from './services/project-admin';
import { ProjectService } from './services/projects';
import { RequestService } from './services/requests';
import { StaffService } from './services/staff';
import { DjService } from './services/dj';
import { TracklistService } from './services/tracklists';
import { SpaceSyncer } from './sync/engine';
import { createProjections } from './sync/projections';

export type App = Awaited<ReturnType<typeof createApp>>;

export type CreateAppOptions = {
	config: Config;
	logger?: Logger;
	db?: Db;
	/** Override how user repo clients are obtained (tests sign in with passwords instead of OAuth). */
	userClient?: (did: string) => Promise<RepoClient | null>;
	/** Start polling, audit retries, and directory sync retries. */
	background?: boolean;
};

/** Wires the whole server side together. Used by SvelteKit hooks and integration tests. */
export async function createApp(options: CreateAppOptions) {
	const { config } = options;

	const logger =
		options.logger ?? createLogger({ level: config.logLevel, pretty: config.dev && config.env !== 'test' });

	const db = options.db ?? (await openDatabase(config.databasePath));

	const identity = new IdentityResolver({
		plcUrl: config.plcUrl,
		handleResolverUrl: config.handleResolverUrl,
		allowPrivateNetwork: config.allowPrivateNetwork,
		logger: logger.child({ component: 'identity' })
	});

	const rootAccount = new RootAccount({ ...config.root, db, identity, logger: logger.child({ component: 'root' }) });
	const rootClient = await rootAccount.connect();
	const rootDid = rootAccount.rootDid;
	const spaces = rootSpaces(rootDid);

	const accounts = new AccountService(db, logger.child({ component: 'accounts' }), identity);
	const oauth = new OAuthService({ config, db, logger: logger.child({ component: 'oauth' }), rootDid });
	const userClient = options.userClient ?? ((did: string) => oauth.repoClient(did));

	const audit = new AuditLog({
		db,
		logger: logger.child({ component: 'audit' }),
		auditSpace: () => spaces.audit,
		rootDid: () => rootDid,
		rootClient: async () => rootClient,
		userClient,
		spacesCapable: (did) => accounts.spacesCapable(did),
		maxAttempts: config.auditMaxAttempts
	});

	const credentials = new SpaceCredentialManager({
		source: async () => rootClient,
		identity,
		logger: logger.child({ component: 'credentials' })
	});

	const core: Core = {
		db,
		logger,
		rootDid,
		spaces,
		bootstrapAdmins: config.adminDids,
		root: async () => rootClient,
		userClient,
		audit,
		accounts,
		// Services only call this from their methods, which run after `syncer` below exists.
		watchSpace: async (space) => {
			await syncer.watch(space);
			void syncer.sync(space).catch(() => undefined);
		},
		fetchSpaceBlob: async (space, repoDid, cid) => {
			const repo = await identity.resolve(repoDid);

			const response = await credentials.request({
				space,
				service: repo.pds,
				audience: repoDid,
				nsid: 'com.atproto.space.getBlob',
				params: { space, repo: repoDid, cid }
			});

			const bytes = new Uint8Array(await response.arrayBuffer());

			return { bytes, mimeType: response.headers.get('content-type') ?? 'application/octet-stream' };
		},
		fetchBlob: async (did, cid, maxBytes) => {
			const url = new URL('/xrpc/com.atproto.sync.getBlob', (await identity.resolve(did)).pds);
			url.searchParams.set('did', did);
			url.searchParams.set('cid', cid);
			const response = await fetch(url, { signal: AbortSignal.timeout(15_000) });

			if (!response.ok) throw new Error(`getBlob ${cid}: HTTP ${response.status}`);

			if (Number(response.headers.get('content-length') ?? 0) > maxBytes) throw new Error(`blob ${cid} is too large`);
			const bytes = new Uint8Array(await response.arrayBuffer());

			if (bytes.length > maxBytes) throw new Error(`blob ${cid} is too large`);

			return { bytes, mimeType: response.headers.get('content-type') ?? 'application/octet-stream' };
		}
	};

	const happyView = new HappyViewClient(config.happyView);
	const projects = new ProjectService(core, happyView);
	const requests = new RequestService(core, projects);
	const staff = new StaffService(core);
	const bookings = new BookingService(core, projects);
	const planner = new PlannerService(core);
	const projectAdmin = new ProjectAdmin(core, projects);
	const tracklists = new TracklistService(core);
	const publicRepo = new PublicRepo(identity, logger.child({ component: 'public-repo' }));
	const dj = new DjService(publicRepo, logger.child({ component: 'dj' }));

	const syncer = new SpaceSyncer({
		db,
		logger: logger.child({ component: 'sync' }),
		identity,
		credentials,
		onChange: createProjections({ core, audit, projects, requests, staff, bookings }),
		notifyService: config.syncNotify ? config.syncService : null
	});

	const sessions = new WebSessions(db, config.sessionDays);

	const geocoder = new Geocoder({
		db,
		logger: logger.child({ component: 'geocode' }),
		provider: config.geocoder,
		userAgent: `LocalNetwork/1.0 (+${config.publicUrl})`
	});

	const timers: NodeJS.Timeout[] = [];
	let bootstrapped = false;
	/** How the last directory sync went. Stays null when this app doesn't manage HappyView. */
	let directorySync: { at: string; error: string | null } | null = null;

	const every = <T>(ms: number, task: () => Promise<T>, name: string) => {
		let running = false;

		const timer = setInterval(() => {
			if (running) return;
			running = true;
			task()
				.catch((error) => logger.warn({ err: error, task: name }, 'background task failed'))
				.finally(() => {
					running = false;
				});
		}, ms);

		timer.unref();
		timers.push(timer);
	};

	const app = {
		config,
		logger,
		db,
		identity,
		rootAccount,
		rootDid,
		spaces,
		core,
		oauth,
		credentials,
		syncer,
		happyView,
		audit,
		accounts,
		projects,
		requests,
		staff,
		bookings,
		planner,
		projectAdmin,
		tracklists,
		dj,
		sessions,
		geocoder,
		limits: {
			login: new RateLimiter(10, 10 / 60),
			write: new RateLimiter(30, 30 / 60),
			search: new RateLimiter(60, 2)
		},

		/**
		 * The account behind a web session, or null when it has to sign in again: it was granted
		 * everything the app asked for at the time, but the app has since asked for more, and
		 * restoring the OAuth session doesn't check that.
		 */
		async viewer(did: string): Promise<Viewer | null> {
			const [account, roles] = await Promise.all([accounts.get(did), staff.roles(did)]);

			if (account?.spacesCapable === 1 && !grantsAll(account.grantedScope, oauth.scopes.full, did)) return null;

			return {
				did,
				handle: account?.handle ?? null,
				roles,
				isAdmin: roles.includes('admin'),
				isStaff: roles.length > 0,
				spacesCapable: account?.spacesCapable === 1
			};
		},

		get directorySync() {
			return directorySync;
		},

		/**
		 * Uploads this app's lexicons and scripts to HappyView where they changed and backfills the root's
		 * projects, when there's an API key. A failure is logged and kept for the System page rather than
		 * thrown: the rest of the app works without the directory, and HappyView keeps its last scripts.
		 */
		async syncDirectory() {
			if (!config.happyView.apiKey) return;

			try {
				const uploaded = await syncHappyView(happyView, happyViewBundle(rootDid), rootDid);
				logger.info(uploaded, 'synced the HappyView directory');
				directorySync = { at: nowIso(), error: null };
			} catch (error) {
				logger.error({ err: error }, 'could not sync the HappyView directory');
				directorySync = { at: nowIso(), error: error instanceof Error ? error.message : String(error) };
			}
		},

		/** Ensures root spaces and starts syncing. Safe to call more than once. */
		async bootstrap() {
			const watched = await ensureRootSpaces(core);

			for (const space of watched) await syncer.watch(space);
			await staff.reconcileMembership();
			bootstrapped = true;
			await syncer.syncAll();
		},

		start() {
			every(config.syncPollIntervalMs, () => (bootstrapped ? syncer.syncAll() : app.bootstrap()), 'sync');
			every(15_000, () => audit.processPending(), 'audit-retry');
			// Retries a failed directory sync, so a HappyView that was down at boot still gets this app's scripts.
			every(
				config.syncPollIntervalMs,
				async () => {
					if (directorySync?.error) await app.syncDirectory();
				},
				'directory-sync'
			);
			every(
				60 * 60_000,
				async () => {
					const cutoff = new Date(Date.now() - 60 * 60_000).toISOString();
					await db.deleteFrom('oauthState').where('updatedAt', '<', cutoff).execute();
				},
				'oauth-state-cleanup'
			);
		},

		async stop() {
			for (const timer of timers) clearInterval(timer);
			await audit.idle();
			await db.destroy();
		}
	};

	await app.syncDirectory();

	if (options.background) {
		app
			.bootstrap()
			.catch((error) => logger.error({ err: error }, 'bootstrap failed; will keep retrying in the background'));
		app.start();
	}

	return app;
}

let instance: Promise<App> | undefined;

/** The process-wide app, created on first use. A failed start is retried on the next call. */
export function getApp(): Promise<App> {
	instance ??= createApp({ config: getConfig(), background: true }).catch((error) => {
		instance = undefined;
		throw error;
	});

	return instance;
}
