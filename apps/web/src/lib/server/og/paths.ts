import type { ProjectView } from '#lib/types.ts';

/**
 * Where each card lives. All but the site's and an artist's carry the CID of the record they're drawn from, so one
 * address always shows the same card and can be cached for good; an edit makes a new address.
 */
export const cardPaths = {
	project: ({ slug, cid }: Pick<ProjectView, 'slug' | 'cid'>) => `/og/project/${slug}/${cid}.png`,
	/** The mix lives in the project's record, so the project's CID names its version. */
	mix: ({ slug, cid }: Pick<ProjectView, 'slug' | 'cid'>, n: number) => `/og/mix/${slug}/${n}/${cid}.png`,
	pressKit: (slug: string, epk: { rkey: string; cid: string }) => `/og/press-kit/${slug}/${epk.rkey}/${epk.cid}.png`,
	artist: (mbid: string) => `/og/artist/${mbid}.png`,
	site: '/og/site.png'
};
