import { type Selectable, type SelectQueryBuilder, type SqlBool, sql } from 'kysely';
import type { Capability } from '#lib/capabilities.ts';
import { parsePerformerUri, performerUri } from '#lib/server/performer-uri.ts';
import type { ActSubject, PerformerRecord, PerformerRef, ProjectView } from '#lib/types.ts';
import { sharedSpace } from '../atproto/spaces';
import type { Core } from '../core';
import { parseJson } from '../db';
import type { Database, PerformerTable } from '../db/schema';
import { ACT_TYPE, DJ_TYPE } from './performer-index';
import type { ProjectService } from './projects';
import { hasCapability, type SharedSpaces } from './shared-spaces';

type PerformerQuery = SelectQueryBuilder<Database, 'performer', Selectable<PerformerTable>>;

/**
 * A performer as the site reads it: an act or a DJ from its space, or a project that hasn't become an act yet, which
 * callers move off one at a time.
 */
export type PerformerView =
	| { kind: 'act'; uri: string; space: string; skey: string; act: ActSubject; formerly: string[]; cid: string }
	| { kind: 'dj'; uri: string; space: string; skey: string; did: string; formerly: string[]; cid: string }
	| { kind: 'project'; uri: string; project: ProjectView };

/**
 * Performers by what names them, and who can act for them. Acts and DJs come from the index of their spaces; a
 * project, from ProjectService, with every manager holding every capability.
 */
export class PerformerService {
	constructor(
		private readonly core: Core,
		private readonly sharedSpaces: SharedSpaces,
		private readonly projects: ProjectService
	) {}

	async byUri(uri: string): Promise<PerformerView | null> {
		const ref = parsePerformerUri(this.core.rootDid, uri);

		return ref && this.byRef(ref);
	}

	async byRef(ref: PerformerRef): Promise<PerformerView | null> {
		switch (ref.kind) {
			case 'act':
				return this.byKey(ref.skey);
			case 'dj':
				return this.forDj(ref.did);
			case 'project': {
				const uri = performerUri(this.core.rootDid, ref);
				const project = await this.projects.byUri(uri);

				return project && { kind: 'project', uri, project };
			}
		}
	}

	/** An act by its slug. */
	async bySlug(slug: string): Promise<PerformerView | null> {
		return this.first((query) => query.where('slug', '=', slug));
	}

	/** An act or a DJ by its space's key. */
	async byKey(skey: string): Promise<PerformerView | null> {
		return this.first((query) => query.where('skey', '=', skey));
	}

	/** The space a DJ keeps their backstage and managers in, once they have one. */
	async forDj(did: string): Promise<PerformerView | null> {
		return this.first((query) => query.where('kind', '=', 'dj').where('did', '=', did));
	}

	/** The performer a project became, found by the project's URI in its `formerly`. */
	async aliases(projectUri: string): Promise<PerformerView | null> {
		return this.first((query) =>
			query.where(sql<SqlBool>`exists (select 1 from json_each(performer.formerly) where value = ${projectUri})`)
		);
	}

	/**
	 * The DIDs of the people who hold `capability` for the performer, as the root's repo has them now: a project's
	 * managers, or the people in an act's or DJ's space who hold it, with a DJ always holding it for themselves. Hidden
	 * people are left out.
	 */
	async holders(ref: PerformerRef, capability: Capability): Promise<string[]> {
		if (ref.kind === 'project') {
			const project = await this.projects.current(performerUri(this.core.rootDid, ref));

			return project?.managers ?? [];
		}

		const performer = await this.byRef(ref);
		const own = ref.kind === 'dj' ? [ref.did] : [];

		if (!performer || performer.kind === 'project') return own;
		const live = await this.sharedSpaces.live(sharedSpace(this.core.rootDid, 'performer', performer.skey));

		const holders = [...live.people.values()].flatMap((person) =>
			!person.hidden && hasCapability(live.kind, person, capability) ? [person.subject] : []
		);

		return [...new Set([...own, ...holders])];
	}

	private async first(filter: (query: PerformerQuery) => PerformerQuery): Promise<PerformerView | null> {
		const row = await filter(this.core.db.selectFrom('performer').selectAll()).executeTakeFirst();

		return row ? this.toView(row) : null;
	}

	private toView(row: Selectable<PerformerTable>): PerformerView | null {
		const record = parseJson<PerformerRecord | null>(row.record, null);
		const formerly = parseJson<string[]>(row.formerly, []);
		const base = { space: row.spaceUri, skey: row.skey, formerly, cid: row.cid };
		const subject = record?.subject;

		if (subject?.$type === ACT_TYPE) {
			const { $type: _type, ...act } = subject;

			return { kind: 'act', uri: performerUri(this.core.rootDid, { kind: 'act', skey: row.skey }), act, ...base };
		}

		if (subject?.$type === DJ_TYPE)
			return {
				kind: 'dj',
				uri: performerUri(this.core.rootDid, { kind: 'dj', did: subject.did }),
				did: subject.did,
				...base
			};

		return null;
	}
}
