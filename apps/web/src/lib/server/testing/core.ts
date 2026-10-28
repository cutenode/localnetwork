import { rootSpaces } from '../atproto/spaces';
import { AuditLog } from '../audit/log';
import { ensureRootSpaces } from '../bootstrap';
import type { Core } from '../core';
import { type Db, openDatabase } from '../db';
import { HappyViewClient } from '../happyview/client';
import { IndexedRepo } from '../happyview/indexed';
import { silentLogger } from '../logger';
import { AccountService } from '../services/accounts';
import { BookingService } from '../services/bookings';
import { EpkService } from '../services/epk';
import { FollowService } from '../services/follows';
import { PlannerService } from '../services/planner';
import { ProjectAdmin } from '../services/project-admin';
import { ProjectService } from '../services/projects';
import { RequestService } from '../services/requests';
import { ShowService } from '../services/shows';
import { StaffService } from '../services/staff';
import { SupportService } from '../services/supports';
import { TracklistService } from '../services/tracklists';
import { VerificationService } from '../services/verifications';
import type { Viewer } from '#lib/types.ts';
import { FakeNetwork } from './fakes';
import { FakeHappyView } from './happyview';

export const ROOT = 'did:plc:root';

/** A fully wired set of services over an in-memory database, a fake PDS network, and a stand-in HappyView. */
export async function createTestServices(options: { admins?: string[]; capable?: string[] } = {}) {
	const db: Db = await openDatabase(':memory:');
	const net = new FakeNetwork();
	const happyView = new FakeHappyView(ROOT);
	// HappyView hears of every public-repo write over Jetstream, and its record script keeps the root's projects.
	net.onRecord = (event) => happyView.ingest(event);
	happyView.repo = (did, collection) => net.listed(did, collection);
	// With an API key, as the app runs, so its re-reads go through the record scripts.
	const directory = new HappyViewClient({ url: 'http://happyview.test', apiKey: 'hv_test', fetch: happyView.fetch });
	const spaces = rootSpaces(ROOT);
	const accounts = new AccountService(db, silentLogger);
	const capable = new Set(options.capable ?? []);

	const audit = new AuditLog({
		db,
		logger: silentLogger,
		auditSpace: () => spaces.audit,
		rootDid: () => ROOT,
		rootClient: async () => net.client(ROOT),
		userClient: async (did) => (capable.has(did) ? net.client(did) : null),
		spacesCapable: async (did) => capable.has(did),
		maxAttempts: 3
	});

	const blobs = new Map<string, { bytes: Uint8Array; mimeType: string }>();

	const core: Core = {
		db,
		logger: silentLogger,
		rootDid: ROOT,
		spaces,
		bootstrapAdmins: options.admins ?? [],
		root: async () => net.client(ROOT),
		userClient: async (did) => (capable.has(did) ? net.client(did) : null),
		audit,
		accounts,
		watchSpace: async () => {},
		fetchSpaceBlob: async (_space, _repo, cid) => {
			const bytes = net.blobs.get(cid);

			if (!bytes) throw new Error('missing blob');

			return blobs.get(cid) ?? { bytes, mimeType: 'image/png' };
		},
		// The root reads a space only once it's a member, as the space host checks its credential.
		readSpaceRecord: async ({ space, repo, collection, rkey }) => {
			const found = net.spaces.get(space);
			const member = space.startsWith(`at://${ROOT}/`) || found?.members.get(ROOT)?.read;

			if (!member) throw new Error(`the root is not a member of ${space}`);

			return net.spaceRecords.get(`${space}|${repo}|${collection}|${rkey}`) ?? null;
		},
		fetchBlob: async (_did, cid) => {
			const bytes = net.blobs.get(cid);

			if (!bytes) throw new Error('missing blob');

			return { bytes, mimeType: 'application/json' };
		}
	};

	const projects = new ProjectService(core, directory);
	const requests = new RequestService(core, projects);
	const staff = new StaffService(core);
	const bookings = new BookingService(core, projects);
	const planner = new PlannerService(core);
	const projectAdmin = new ProjectAdmin(core, projects);
	const tracklists = new TracklistService(core);
	const supports = new SupportService(directory, accounts, silentLogger);

	// Writes are indexed in-process here, so a write that isn't served at once never will be.
	const indexedRepo = new IndexedRepo(directory, silentLogger, 0);

	const epks = new EpkService(core, projects, indexedRepo, async (identifier) => {
		if (identifier.startsWith('did:')) return identifier;
		throw new Error(`no such handle ${identifier}`);
	});

	const shows = new ShowService(core, projects, directory, indexedRepo, 0);
	const follows = new FollowService(core, directory, 0);

	/** Each account's handle, as the identity resolver would report it once checked both ways. */
	const handles = new Map<string, string>();

	const verifications = new VerificationService(core, indexedRepo, projects, {
		resolve: async (did) => ({ handle: handles.get(did) ?? null })
	});

	await ensureRootSpaces(core);
	await audit.idle();

	const viewer = async (did: string): Promise<Viewer> => {
		const roles = await staff.roles(did);

		return {
			did,
			handle: null,
			roles,
			isAdmin: roles.includes('admin'),
			isStaff: roles.length > 0,
			spacesCapable: capable.has(did)
		};
	};

	return {
		db,
		net,
		happyView,
		directory,
		core,
		audit,
		accounts,
		projects,
		requests,
		staff,
		bookings,
		planner,
		projectAdmin,
		tracklists,
		supports,
		epks,
		shows,
		follows,
		verifications,
		handles,
		indexedRepo,
		viewer,
		capable,
		spaces
	};
}
