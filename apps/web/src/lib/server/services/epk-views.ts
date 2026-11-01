import { createHmac, randomBytes } from 'node:crypto';
import type { EpkViewReceipt } from '#lib/epk.ts';
import type { ProjectView, Viewer } from '#lib/types.ts';
import { parsePublicRecordUri } from '../atproto/spaces';
import { type Core, NotFoundError } from '../core';
import type { EpkService, PressKit } from './epk';
import { actorRole } from './requests';

/** Views by the same person of the same press kit closer together than this count as one, so a reload isn't a receipt. */
const VIEW_GAP_MS = 30 * 60_000;

/** The most receipts a project's dashboard lists. */
const LIST_LIMIT = 200;

const DAY_MS = 86_400_000;

/** The HMAC of `did` under `salt`, which a view's audit entry holds in place of who viewed. */
export const viewerCommitment = (salt: string, did: string) => createHmac('sha256', salt).update(did).digest('hex');

/**
 * Receipts of who viewed a press kit's private part, for its project's managers. Each view goes into the audit chain
 * with the root as its actor and an HMAC of the viewer's DID under a random salt, never the DID itself; the DID and the
 * salt stay in this app's database. Deleting a receipt (a manager can, and every receipt expires after the retention
 * period) drops both, so the chain still verifies but no longer says who viewed. While a receipt exists, its salt and
 * DID reproduce the HMAC in the chain.
 */
export class EpkViews {
	constructor(
		private readonly core: Core,
		private readonly epks: Pick<EpkService, 'assertManager'>,
		/** How long receipts are kept. */
		readonly retentionDays: number,
		private readonly now: () => number = Date.now
	) {}

	/**
	 * Records that `viewer`, who isn't a manager, saw the private part of `kit`. A failure is logged, never thrown, so a
	 * press kit never fails to open over its receipt.
	 */
	async record(kit: PressKit, viewer: Viewer): Promise<void> {
		const { epk, project } = kit;

		try {
			const since = new Date(this.now() - VIEW_GAP_MS).toISOString();

			const recent = await this.core.db
				.selectFrom('epkView')
				.select('id')
				.where('epkUri', '=', epk.uri)
				.where('viewerDid', '=', viewer.did)
				.where('createdAt', '>', since)
				.executeTakeFirst();

			if (recent) return;
			const createdAt = new Date(this.now()).toISOString();
			const salt = randomBytes(32).toString('hex');

			const entry = await this.core.audit.record({
				actor: this.core.rootDid,
				actorRole: 'system',
				action: 'epk.view',
				subject: epk.uri,
				subjectKind: 'epk',
				summary: `Someone with access viewed the press kit “${epk.record.title}”`,
				details: { viewer: viewerCommitment(salt, viewer.did) }
			});

			await this.core.db
				.insertInto('epkView')
				.values({
					id: entry.id,
					epkUri: epk.uri,
					epkTitle: epk.record.title,
					projectUri: project.uri,
					viewerDid: viewer.did,
					salt,
					createdAt
				})
				.execute();
			this.core.logger.debug({ epk: epk.uri, entry: entry.id }, 'recorded a press kit view');
		} catch (error) {
			this.core.logger.warn({ err: error, epk: epk.uri }, 'could not record a press kit view');
		}
	}

	/** The project's receipts still within the retention period, newest first. */
	async list(project: ProjectView): Promise<EpkViewReceipt[]> {
		const rows = await this.core.db
			.selectFrom('epkView')
			.select(['id', 'epkUri', 'epkTitle', 'viewerDid', 'createdAt'])
			.where('projectUri', '=', project.uri)
			.where('createdAt', '>=', this.cutoff())
			.orderBy('createdAt', 'desc')
			.limit(LIST_LIMIT)
			.execute();

		const accounts = await this.core.accounts.views(rows.map((row) => row.viewerDid));

		return rows.map((row) => ({
			id: row.id,
			viewer: accounts.get(row.viewerDid) ?? { did: row.viewerDid, handle: null },
			epk: { uri: row.epkUri, rkey: parsePublicRecordUri(row.epkUri)?.rkey ?? '', title: row.epkTitle },
			viewedAt: row.createdAt
		}));
	}

	/** One of the project's managers deletes a receipt, which the audit log records without saying whose it was. */
	async remove(actor: Viewer, project: ProjectView, id: string): Promise<void> {
		await this.epks.assertManager(actor, project);

		const row = await this.core.db
			.deleteFrom('epkView')
			.where('id', '=', id)
			.where('projectUri', '=', project.uri)
			.returning(['epkUri', 'epkTitle'])
			.executeTakeFirst();

		if (!row) throw new NotFoundError('That view is already gone');
		await this.core.audit.record({
			actor: actor.did,
			actorRole: actorRole(actor),
			action: 'epk.forget',
			subject: row.epkUri,
			subjectKind: 'epk',
			summary: `Deleted a view receipt of the press kit “${row.epkTitle}”`,
			details: { entry: id }
		});
	}

	/** Deletes the receipts past the retention period. Returns how many went. */
	async expire(): Promise<number> {
		const result = await this.core.db.deleteFrom('epkView').where('createdAt', '<', this.cutoff()).executeTakeFirst();
		const expired = Number(result.numDeletedRows);

		if (expired) this.core.logger.info({ expired }, 'expired press kit view receipts');

		return expired;
	}

	private cutoff(): string {
		return new Date(this.now() - this.retentionDays * DAY_MS).toISOString();
	}
}
