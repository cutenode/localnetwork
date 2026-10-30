import { USERINPUT } from '#lib/bug-report.ts';
import { BSKY_FOLLOW, COLLECTION, VERIFICATION } from '#lib/nsid.ts';
import type { SpaceKind } from '#lib/capabilities.ts';
import type { NotificationChannel, Viewer } from '#lib/types.ts';
import { SpaceCredentialManager } from './atproto/credentials';
import { IdentityResolver } from './atproto/identity';
import { grantsSpaces, OAuthService } from './atproto/oauth';
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
import { Backfills } from './happyview/backfills';
import { happyViewBundle } from './happyview/bundle';
import { HappyViewClient } from './happyview/client';
import { INDEX_WAIT_MS, IndexedRepo } from './happyview/indexed';
import { MANAGER_COLLECTIONS, refreshForScript } from './happyview/refresh';
import { syncHappyView } from './happyview/sync';
import { createLogger, type Logger } from './logger';
import { NotificationChannels } from './notifications/channels';
import { confirmationSender, emailChannel, LogEmailSender, ResendEmailSender } from './notifications/email';
import { Notifier } from './notifications/notifier';
import { dmChannel, LogDmSender, RootDmSender } from './notifications/dm';
import { type ChannelSender, Outbox } from './notifications/outbox';
import { RateLimiter } from './rate-limit';
import { AccountService } from './services/accounts';
import { BookingService } from './services/bookings';
import { BugReportService } from './services/bug-reports';
import { DjService } from './services/dj';
import { DjListings } from './services/dj-listings';
import { ProfileService } from './services/profiles';
import { EpkService } from './services/epk';
import { FollowService } from './services/follows';
import { PlannerService } from './services/planner';
import { ProjectAdmin } from './services/project-admin';
import { ProjectService } from './services/projects';
import { RequestService } from './services/requests';
import { type ListingSource, ListingWriter } from './services/listings';
import { SharedSpaces } from './services/shared-spaces';
import { SpaceJoins } from './services/space-joins';
import { ShowService } from './services/shows';
import { StaffService } from './services/staff';
import { SupportService } from './services/supports';
import { TracklistService } from './services/tracklists';
import { VerificationService } from './services/verifications';
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
	/** What each kind's listing shows. No kind has a public page yet, so only tests list anything. */
	listingSources?: Partial<Record<SpaceKind, ListingSource>>;
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
	const oauth = new OAuthService({ config, db, logger: logger.child({ component: 'oauth' }) });
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

	const { email } = config;

	const emailSender =
		email.provider === 'resend'
			? new ResendEmailSender({ apiKey: email.apiKey, from: email.from })
			: email.provider === 'log'
				? new LogEmailSender(logger.child({ component: 'email' }))
				: null;

	const notificationChannels = new NotificationChannels({
		db,
		logger: logger.child({ component: 'notifications' }),
		sendConfirmation: emailSender
			? confirmationSender(emailSender, config.publicUrl, logger.child({ component: 'email' }))
			: undefined,
		limits: {
			account: new RateLimiter(5, 5 / 86_400),
			accountAddress: new RateLimiter(3, 3 / 86_400),
			address: new RateLimiter(10, 10 / 86_400),
			all: new RateLimiter(100, 100 / 3600)
		}
	});

	const dmLogger = logger.child({ component: 'dm' });

	const dm =
		config.dm.provider === 'bluesky'
			? new RootDmSender({
					chat: () => rootAccount.chat(config.dm.chatService),
					bluesky: () => rootAccount.bluesky,
					logger: dmLogger
				})
			: config.dm.provider === 'log'
				? new LogDmSender(dmLogger)
				: null;

	const senders: Partial<Record<NotificationChannel, ChannelSender>> = {};

	if (emailSender) senders.email = emailChannel(emailSender, config.publicUrl);

	if (dm) senders.dm = dmChannel(dm, config.publicUrl);

	const outbox = new Outbox({
		db,
		logger: logger.child({ component: 'outbox' }),
		channels: notificationChannels,
		senders
	});

	const notifications = new Notifier({ db, logger: logger.child({ component: 'notifications' }), outbox });

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
		notify: notifications.notify.bind(notifications),
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
		readSpaceRecord: (input) => credentials.readRecord(input),
		fetchBlob: async (did, cid, maxBytes) => {
			// Blobs come from the account's PDS until HappyView serves them.
			// ast-grep-ignore: no-direct-pds-read
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
	const bundle = () => happyViewBundle(rootDid, config.userinputSpace);
	const backfills = new Backfills(happyView, logger.child({ component: 'backfills' }));
	const projects = new ProjectService(core, happyView, INDEX_WAIT_MS, backfills);
	const staff = new StaffService(core);
	const bookings = new BookingService(core, projects);
	const planner = new PlannerService(core);
	const projectAdmin = new ProjectAdmin(core, projects);
	const tracklists = new TracklistService(core);
	const indexedRepo = new IndexedRepo(happyView, logger.child({ component: 'indexed-repo' }));
	const dj = new DjService(indexedRepo, logger.child({ component: 'dj' }));
	const profiles = new ProfileService(core, indexedRepo);
	const djListings = new DjListings(core, indexedRepo);
	const requests = new RequestService(core, projects, profiles, djListings);
	const supports = new SupportService(happyView, accounts, logger.child({ component: 'supports' }));

	const epks = new EpkService(core, projects, indexedRepo, (identifier) => identity.resolveIdentifier(identifier));

	const shows = new ShowService(core, projects, happyView, indexedRepo);
	const follows = new FollowService(core, happyView);
	const verifications = new VerificationService(core, indexedRepo, projects, identity);
	const listings = new ListingWriter(core, options.listingSources ?? {});
	const sharedSpaces = new SharedSpaces(core, listings);

	const limits = {
		login: new RateLimiter(10, 10 / 60),
		write: new RateLimiter(30, 30 / 60),
		search: new RateLimiter(60, 2),
		/** Profile addresses that name no account, each of which may have sent the app to resolve a stranger's name. */
		profileMisses: new RateLimiter(30, 30 / 600),
		/** Turning email or DMs on, or checking again, which asks other services each time. */
		optIn: new RateLimiter(5, 5 / 600),
		testSend: new RateLimiter(3, 3 / 3600)
	};

	const spaceJoins = new SpaceJoins(core, sharedSpaces, limits.write);

	const bugReports = new BugReportService({
		space: config.userinputSpace,
		indexed: indexedRepo,
		userClient,
		accounts,
		logger: logger.child({ component: 'bug-reports' })
	});

	const syncer = new SpaceSyncer({
		db,
		logger: logger.child({ component: 'sync' }),
		identity,
		credentials,
		onChange: createProjections({ core, audit, projects, requests, staff, bookings, epks, sharedSpaces, listings }),
		onSpaceDropped: async (space) => {
			await bookings.forget(space);
			await sharedSpaces.forget(space);
		},
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
		backfills,
		audit,
		accounts,
		notifications,
		notificationChannels,
		outbox,
		/** Bluesky DMs from the root, or null when this site doesn't send them. */
		dm,
		projects,
		sets: projects.sets,
		requests,
		staff,
		bookings,
		planner,
		projectAdmin,
		tracklists,
		dj,
		profiles,
		djListings,
		supports,
		epks,
		shows,
		follows,
		verifications,
		sharedSpaces,
		spaceJoins,
		listings,
		bugReports,
		sessions,
		geocoder,
		limits,

		/**
		 * The account behind a web session, or null when it has to sign in again: it was granted
		 * everything the app asked for at the time, but the app has since asked for more, and
		 * restoring the OAuth session doesn't check that.
		 */
		async viewer(did: string): Promise<Viewer | null> {
			const [account, roles] = await Promise.all([accounts.get(did), staff.roles(did)]);

			if (account?.spacesCapable === 1 && !grantsSpaces(account.grantedScope, did)) return null;

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
		 * projects, then each manager's shows and (once per record script) the older records the site reads from other
		 * repos, when there's an API key. A failure is logged and kept for the System page rather than thrown: the rest of
		 * the app works without the directory, and HappyView keeps its last scripts.
		 */
		async syncDirectory() {
			if (!config.happyView.apiKey) return;

			try {
				const uploaded = await syncHappyView(happyView, bundle(), rootDid);
				logger.info(uploaded, 'synced the HappyView directory');
				directorySync = { at: nowIso(), error: null };
				// In the background: the shows Jetstream missed don't hold the app up.
				void shows.reindexManagers().catch((error) => logger.warn({ err: error }, 'could not re-read shows'));
				await app.refreshRecords(uploaded.scripts);
			} catch (error) {
				logger.error({ err: error }, 'could not sync the HappyView directory');
				directorySync = { at: nowIso(), error: error instanceof Error ? error.message : String(error) };
			}
		},

		/**
		 * Has HappyView re-read, once for each collection's current record script (`uploaded` lists the scripts it just
		 * got), the older records the site reads from other repos: the records of each of MANAGER_COLLECTIONS by the
		 * managers and the root (which holds press kits), the follows of the accounts press kits' audience checks read, the
		 * root's verifications and DJ listings, and the bug report space. A failure is logged and tried again on the next
		 * round, without marking the directory sync failed.
		 */
		async refreshRecords(uploaded: string[] = []) {
			if (!config.happyView.apiKey) return;
			const { scripts } = bundle();
			let managers: Promise<string[]> | undefined;
			const spaceOwner = config.userinputSpace?.split('/')[2];

			const refreshes = [
				...MANAGER_COLLECTIONS.map((collection) => ({
					collection,
					accounts: () => (managers ??= projects.managers().then((dids) => [rootDid, ...dids]))
				})),
				{ collection: BSKY_FOLLOW, accounts: () => epks.followReaders() },
				{ collection: VERIFICATION, accounts: async () => [rootDid] },
				{ collection: COLLECTION.listingProfile, accounts: async () => [rootDid] },
				...(spaceOwner ? [{ collection: USERINPUT.space, accounts: async () => [spaceOwner] }] : [])
			];

			for (const refresh of refreshes) {
				const id = `record.index:${refresh.collection}`;
				const script = scripts.find((candidate) => candidate.id === id)!.body;

				try {
					await refreshForScript(core, happyView, { ...refresh, script, uploaded: uploaded.includes(id) });
				} catch (error) {
					logger.warn({ err: error, collection: refresh.collection }, 'could not have HappyView re-read older records');
				}
			}
		},

		/** Ensures root spaces and starts syncing. Safe to call more than once. */
		async bootstrap() {
			const watched = await ensureRootSpaces(core);

			for (const space of watched) await syncer.watch(space);
			await staff.reconcileMembership();
			await projects.reconcileManagerSpaces();
			await sharedSpaces.reconcile();
			bootstrapped = true;
			await syncer.syncAll();
			// After the sync, so the index has every space's people.
			await listings.reconcile();
		},

		start() {
			every(config.syncPollIntervalMs, () => (bootstrapped ? syncer.syncAll() : app.bootstrap()), 'sync');
			every(15_000, () => audit.processPending(), 'audit-retry');
			// Retries a failed directory sync, so a HappyView that was down at boot still gets this app's scripts, and
			// the older records until HappyView has re-read them all.
			every(
				config.syncPollIntervalMs,
				async () => {
					if (directorySync?.error) await app.syncDirectory();
					else if (directorySync) await app.refreshRecords();
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
			// Finishes manager syncs that failed or raced a new thread. Costs one listRecords of the root's projects and
			// one listMembers per booking thread, all on the root's own PDS, and writes only where something drifted.
			every(60 * 60_000, () => projects.reconcileManagerSpaces(), 'manager-spaces');
			// Finishes member updates of performers', parties' and venues' spaces that failed part way: a listSpaces per
			// kind, then one listRecords and one listMembers per space, all on the root's own PDS.
			every(60 * 60_000, () => sharedSpaces.reconcile(), 'shared-spaces');
			// Rewrites listings that drifted from their spaces: one listRecords of the root's listings, and writes only
			// where one differs.
			every(60 * 60_000, () => listings.reconcile(), 'listings');
			every(60 * 60_000, () => Promise.all([notifications.prune(), outbox.prune()]), 'inbox-retention');
			every(15_000, () => outbox.processDue(), 'notification-delivery');
			void outbox
				.recover()
				.then(() => outbox.processDue())
				.catch((error) => logger.warn({ err: error }, 'could not resume notification delivery'));
		},

		async stop() {
			for (const timer of timers) clearInterval(timer);
			outbox.stop();
			await Promise.all([audit.idle(), outbox.idle()]);
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
