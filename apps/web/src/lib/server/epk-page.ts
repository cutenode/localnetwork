import { error } from '@sveltejs/kit';
import type { EpkPhoto } from '#lib/epk.ts';
import type { ProjectView, Viewer } from '#lib/types.ts';
import { parseEpkForm } from './forms/epk';
import type { EpkService } from './services/epk';

/** A press kit by its owner and key, or a 404. */
export async function loadEpk(epks: EpkService, did: string, rkey: string) {
	const found = await epks.get(did, rkey);

	if (!found) error(404, 'Press kit not found');

	return found;
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
		rkey
	);
}
