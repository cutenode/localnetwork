import type { PerformerResult } from '#lib/types.ts';
import type { Core } from '../core';
import type { HappyViewClient } from '../happyview/client';
import type { ProfileService } from './profiles';
import { type ProjectSearch, type ProjectService, searchQuery } from './projects';

export type PerformerSearchResult = { items: PerformerResult[]; total: number; page: number; pageSize: number };

/**
 * Find DJs: active projects and the DJs staff list, searched together by the directory (`searchPerformers`), with each
 * DJ's handle from the accounts this app knows, for links to their profile.
 */
export class PerformerSearch {
	constructor(
		private readonly core: Pick<Core, 'accounts'>,
		private readonly directory: HappyViewClient,
		private readonly projects: ProjectService,
		private readonly profiles: ProfileService
	) {}

	async search(params: Omit<ProjectSearch, 'manager' | 'status' | 'includeInactive'>): Promise<PerformerSearchResult> {
		const { page, pageSize, query } = searchQuery(params);
		const output = await this.directory.query('searchPerformers', query);
		const dids = output.performers.flatMap((found) => ('profile' in found ? [found.profile.uri.split('/')[2]] : []));
		const accounts = await this.core.accounts.views(dids);

		const items = output.performers.flatMap((found): PerformerResult[] => {
			if ('project' in found) {
				const project = this.projects.toView(found.project);

				return project ? [{ kind: 'project', project }] : [];
			}

			const { uri, cid, record, genres, distanceM } = found.profile;
			const did = uri.split('/')[2];
			const profile = this.profiles.toView(did, uri, cid, record);

			if (!profile) return [];

			return [
				{
					kind: 'dj',
					dj: {
						profile,
						handle: accounts.get(did)?.handle ?? null,
						genres,
						...(distanceM !== undefined && { distanceKm: distanceM / 1000 })
					}
				}
			];
		});

		return { items, total: output.total, page, pageSize };
	}
}
