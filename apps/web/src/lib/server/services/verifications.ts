import { z } from 'zod';
import { BSKY_PROFILE, VERIFICATION } from '#lib/nsid.ts';
import type { IssuedVerification, LiveIdentity, VerificationStatus, Viewer } from '#lib/types.ts';
import { isCurrent } from '#lib/verification.ts';
import { validateRecord } from '../atproto/lexicons';
import { type RecordRef, xrpcErrorName } from '../atproto/repo-client';
import { type Core, ForbiddenError, UserError } from '../core';
import { nowIso } from '../db';
import type { IndexedRepo } from '../happyview/indexed';
import type { ProjectService } from './projects';
import { actorRole } from './requests';

const verificationRecord = z.object({
	subject: z.string(),
	handle: z.string(),
	displayName: z.string(),
	createdAt: z.string()
});

/** The part of a Bluesky profile (`app.bsky.actor.profile`) a verification copies. */
const bskyProfile = z.object({ displayName: z.string().optional() });

/** The handle a DID claims, or null unless that handle points back to it: the app's IdentityResolver. */
type HandleResolver = { resolve(did: string): Promise<{ handle: string | null }> };

/**
 * Bluesky verifications issued from the root account. Apps that trust the root show a badge on the account for as
 * long as its handle and display name match the ones verified, so a rename needs a fresh one. Both are read from
 * HappyView, which keeps the root's verifications (lua/app/bsky/graph/verification.lua) and the display names of the
 * accounts they and the root's projects name (lua/app/bsky/actor/profile.lua).
 */
export class VerificationService {
	constructor(
		private readonly core: Core,
		private readonly indexed: IndexedRepo,
		private readonly projects: Pick<ProjectService, 'managedBy'>,
		private readonly identity: HandleResolver
	) {}

	/**
	 * Every verification the root has issued, newest first. A HappyView that's down fails this rather than reading as
	 * none, which would show everyone as unverified and let a grant write a second verification of an account.
	 */
	async list(): Promise<IssuedVerification[]> {
		return (await this.indexed.listAll(this.core.rootDid, VERIFICATION, { strict: true }))
			.flatMap((record) => {
				const parsed = verificationRecord.safeParse(record.value);

				if (!parsed.success) return [];

				return [{ uri: record.uri, cid: record.cid, rkey: record.uri.split('/').at(-1) ?? '', ...parsed.data }];
			})
			.toSorted((a, b) => b.createdAt.localeCompare(a.createdAt));
	}

	/**
	 * An account's handle and display name as they are now. One that can't be read comes back null. `strict` is for a
	 * grant, which shouldn't read a HappyView that's down as an account without a display name.
	 */
	async live(did: string, { strict = false } = {}): Promise<LiveIdentity> {
		const [identity, profile] = await Promise.all([
			this.identity.resolve(did).catch(() => null),
			// HappyView re-reads one it doesn't have yet, in case Jetstream brought no change since the account came to
			// manage a project or since HappyView kept profiles.
			this.indexed.getRecordOrReindex(did, BSKY_PROFILE, 'self', { strict })
		]);

		const displayName = bskyProfile.safeParse(profile?.value).data?.displayName ?? null;

		return { handle: identity?.handle ?? null, displayName };
	}

	/**
	 * Where each of `dids` stands, for the shortcuts on claims and project managers. Null when the root's verifications
	 * can't be read, so those pages leave the shortcut out instead of failing.
	 */
	async statuses(dids: string[]): Promise<Record<string, VerificationStatus> | null> {
		if (dids.length === 0) return {};

		const issued = await this.list().catch((error) => {
			this.core.logger.warn({ err: error }, 'could not list verifications');

			return null;
		});

		if (!issued) return null;

		const entries = await Promise.all(
			dids.map(async (did): Promise<[string, VerificationStatus]> => {
				const verification = issued.find((item) => item.subject === did);

				if (!verification) return [did, 'unverified'];

				return [did, isCurrent(verification, await this.live(did)) ? 'verified' : 'outdated'];
			})
		);

		return Object.fromEntries(entries);
	}

	/**
	 * Verifies `subject` under its current handle and display name, replacing an earlier verification of it in place:
	 * apps count one per issuer and account. Only a project manager, or an account verified already, can be: HappyView
	 * keeps no one else's display name.
	 */
	async grant(actor: Viewer, subject: string): Promise<{ handle: string; displayName: string }> {
		await this.requireAdmin(actor, 'verification.grant', subject);
		const previous = (await this.list()).find((item) => item.subject === subject);

		if (!previous && (await this.projects.managedBy(subject)).length === 0)
			throw new UserError('Only project managers can be verified');
		const { handle, displayName } = await this.live(subject, { strict: true });

		if (!handle) throw new UserError("That account's handle doesn't point back to it, so it can't be verified");

		if (!displayName) throw new UserError(`@${handle} has no display name. Ask them to set one, then verify them`);

		if (previous && isCurrent(previous, { handle, displayName })) throw new UserError(`@${handle} is already verified`);

		const record = validateRecord(VERIFICATION, { subject, handle, displayName, createdAt: nowIso() });
		const root = await this.core.root();
		let ref: RecordRef;

		try {
			// Swaps on the version HappyView serves, so it can't replace one this page hasn't shown.
			ref = previous
				? await root.putRecord({ collection: VERIFICATION, rkey: previous.rkey, record, swapRecord: previous.cid })
				: await root.createRecord({ collection: VERIFICATION, record });
		} catch (error) {
			if (xrpcErrorName(error) !== 'InvalidSwap') throw error;
			// Changed somewhere HappyView hasn't caught up with yet, so a reload would show the same old version.
			await this.indexed.reindex(this.core.rootDid, VERIFICATION);
			throw new UserError(`@${handle}'s verification changed since this page loaded. Reload and try again.`, 409);
		}

		await this.indexed.awaitWrite(this.core.rootDid, VERIFICATION, ref.uri.split('/').at(-1) ?? '', ref.cid);

		await this.core.audit.record({
			actor: actor.did,
			actorRole: 'admin',
			action: 'verification.grant',
			subject,
			subjectKind: 'account',
			summary: `${previous ? 'Re-verified' : 'Verified'} @${handle} on Bluesky`,
			changes: [
				{ field: 'handle', before: previous?.handle, after: handle },
				{ field: 'displayName', before: previous?.displayName, after: displayName }
			],
			details: { uri: ref.uri }
		});

		return { handle, displayName };
	}

	/** Deletes every verification of `subject`, which removes the badge wherever it showed. */
	async revoke(actor: Viewer, subject: string): Promise<void> {
		await this.requireAdmin(actor, 'verification.revoke', subject);
		const issued = (await this.list()).filter((item) => item.subject === subject);

		if (issued.length === 0) throw new UserError("That account isn't verified");
		const root = await this.core.root();

		for (const verification of issued) {
			await root.deleteRecord({ collection: VERIFICATION, rkey: verification.rkey });
			await this.indexed.awaitWrite(this.core.rootDid, VERIFICATION, verification.rkey, null);
		}

		const [latest] = issued;
		await this.core.audit.record({
			actor: actor.did,
			actorRole: 'admin',
			action: 'verification.revoke',
			subject,
			subjectKind: 'account',
			summary: `Removed the Bluesky verification of @${latest.handle}`,
			changes: [
				{ field: 'handle', before: latest.handle },
				{ field: 'displayName', before: latest.displayName }
			]
		});
	}

	/** Verifications speak for Local Network wherever they show, so only admins issue them. */
	private async requireAdmin(actor: Viewer, action: 'verification.grant' | 'verification.revoke', subject: string) {
		if (actor.isAdmin) return;
		await this.core.audit.record({
			actor: actor.did,
			actorRole: actorRole(actor),
			action,
			subject,
			subjectKind: 'account',
			summary: 'Denied: only admins can manage verifications',
			outcome: 'denied'
		});
		throw new ForbiddenError('Only admins can manage verifications');
	}
}
