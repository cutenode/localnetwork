import { error } from '@sveltejs/kit';
import type { EpkPhoto } from '#lib/epk.ts';
import type { ProjectView, Viewer } from '#lib/types.ts';
import { parseEpkForm } from './forms/epk';
import { requireManager } from './manage';
import type { EpkService } from './services/epk';
import type { ProjectService } from './services/projects';

/** A press kit by its owner and key, or a 404. */
export async function loadEpk(epks: EpkService, did: string, rkey: string) {
	const found = await epks.get(did, rkey);

	if (!found) error(404, 'Press kit not found');

	return found;
}

/**
 * A press kit of project `slug` that the viewer made, for its dashboard pages: a 403 for the project's
 * other managers, who see it listed but can't change it, and a 404 when it's another project's.
 */
export async function ownedEpk(
	locals: { viewer: Viewer | null; app: { projects: Pick<ProjectService, 'bySlug'>; epks: EpkService } },
	url: URL,
	params: { slug: string; did: string; rkey: string }
) {
	const { viewer, project } = await requireManager(locals, url, params.slug);

	if (viewer.did !== params.did) error(403, 'Only the manager who made this press kit can change it');
	const found = await loadEpk(locals.app.epks, params.did, params.rkey);

	if (found.project.uri !== project.uri) error(404, 'Press kit not found');

	return { viewer, ...found };
}

/** Saves the press kit form: new photos go to the owner's repo first, then the records. */
export async function saveEpkForm(
	epks: EpkService,
	viewer: Viewer,
	project: ProjectView,
	form: FormData,
	existingPhotos: EpkPhoto[],
	rkey?: string
) {
	const parsed = await parseEpkForm(form, project.profile.mixes ?? [], existingPhotos);
	const publicPhotos = [...parsed.publicPhotos];
	const privatePhotos = [...parsed.privatePhotos];

	for (const upload of parsed.uploads) {
		const photo: EpkPhoto = {
			image: await epks.uploadPhoto(viewer, upload.bytes, upload.mimeType),
			alt: `${project.profile.name}, press photo`
		};

		if (upload.credit) photo.credit = upload.credit;

		if (upload.aspectRatio) photo.aspectRatio = upload.aspectRatio;
		(upload.visibility === 'public' ? publicPhotos : privatePhotos).push(photo);
	}

	return epks.save(
		viewer,
		project,
		{
			title: parsed.title,
			headline: parsed.headline,
			bio: parsed.bio,
			access: parsed.access,
			mixes: parsed.publicMixes,
			photos: publicPhotos,
			content: { ...parsed.content, mixes: parsed.privateMixes, photos: privatePhotos }
		},
		rkey ? { rkey, cid: parsed.cid } : undefined
	);
}
