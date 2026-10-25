import Bowser from 'bowser';
import { discussionUrl, USERINPUT } from '#lib/bug-report.ts';
import { validateRecord } from '../atproto/lexicons';
import { grantsAll, SCOPES } from '../atproto/oauth';
import type { RepoClient } from '../atproto/repo-client';
import type { IndexedRepo } from '../happyview/indexed';
import type { Logger } from '../logger';
import type { AccountService } from './accounts';

/**
 * "Chrome 141, macOS" from a User-Agent header, or undefined when it names no browser. Here rather than in
 * #lib/bug-report.ts, which the report form imports, so the parser stays on the server.
 */
export function browserLabel(userAgent: string | null | undefined): string | undefined {
	if (!userAgent) return undefined;
	const { browser, os } = Bowser.parse(userAgent);

	if (!browser.name) return undefined;
	const major = browser.version?.split('.')[0];

	// Browsers freeze the OS version in the User-Agent (Chrome says macOS 10.15 on every Mac), so only its name goes in.
	return [[browser.name, major].filter(Boolean).join(' '), os.name].filter(Boolean).join(', ');
}

/**
 * Bug reports from signed-in people, posted from their own account into Local Network's space on
 * userinput.app (a feedback board on the AT Protocol). Without a space configured, the site sends
 * everyone to GitHub's issue form instead.
 */
export class BugReportService {
	constructor(
		private readonly deps: {
			/** The space's at:// URI. */
			space: string | undefined;
			/** Where the space is read: HappyView keeps it (lua/app/userinput/space.lua). */
			indexed: IndexedRepo;
			userClient: (did: string) => Promise<RepoClient | null>;
			accounts: AccountService;
			logger: Logger;
		}
	) {}

	get enabled(): boolean {
		return this.deps.space !== undefined;
	}

	/** Whether `did` approved posting to userinput.app. Sessions from before the site asked for it haven't. */
	async canPost(did: string): Promise<boolean> {
		return grantsAll((await this.deps.accounts.get(did))?.grantedScope, SCOPES.bugReport, did);
	}

	/** Posts a report as `did` and returns where userinput.app shows it. */
	async post(did: string, report: { title: string; body: string }): Promise<string> {
		const { space, indexed, userClient, logger } = this.deps;

		if (!space) throw new Error('no userinput.app space is configured');
		const [owner, , rkey] = space.replace('at://', '').split('/');
		// A post points at one version of the space, so its CID is looked up each time.
		const found = await indexed.getRecordOrReindex(owner, USERINPUT.space, rkey, { strict: true });

		if (!found?.cid) throw new Error(`userinput.app space ${space} is missing`);
		const client = await userClient(did);

		if (!client) throw new Error(`no OAuth session for ${did}`);

		// Tagged with the space's own tags for a website bug.
		const record = validateRecord(USERINPUT.discussion, {
			space: { uri: found.uri, cid: found.cid },
			title: report.title,
			body: report.body,
			tags: ['bug', 'web'],
			createdAt: new Date().toISOString()
		});

		const { uri } = await client.createRecord({ collection: USERINPUT.discussion, record });
		logger.info({ did, uri }, 'posted a bug report to userinput.app');

		return discussionUrl(uri);
	}
}
