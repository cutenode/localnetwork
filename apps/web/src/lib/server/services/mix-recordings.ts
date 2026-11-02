import { plyrTrackId } from '#lib/mix/links.ts';
import { PLYR_TRACK } from '#lib/mix/source.ts';
import type { ProjectMix, ProjectProfile, StrongRef } from '#lib/types.ts';
import { parsePublicRecordUri } from '../atproto/spaces';
import { CID } from '../http';
import type { PlyrClient } from '../plyr';

/** Whether `ref` is an `fm.plyr.track` in one of `managers`' repos. */
const isManagersTrack = (ref: StrongRef, managers: string[]) => {
	const parts = parsePublicRecordUri(ref.uri);

	return parts?.collection === PLYR_TRACK && managers.includes(parts.did) && CID.test(ref.cid);
};

/**
 * Sets each mix's `recording` before root publishes `profile`: for a mix whose link is a plyr.fm track, the track's
 * `fm.plyr.track` record, when it's in one of `managers`' repos, so the DJ hosts the audio. A link `published` already
 * has keeps its recording, pinned to that version like a linked set, so plyr.fm is only asked about new links. Any
 * other mix, or one plyr.fm can't answer for, has none and plays its link. A recording the caller passes is ignored,
 * since it's only ever worked out here.
 */
export async function withRecordings(
	profile: ProjectProfile,
	published: ProjectProfile | undefined,
	managers: string[],
	plyr: PlyrClient | undefined
): Promise<ProjectProfile> {
	if (!profile.mixes) return profile;

	const recordingOf = async (url: string): Promise<StrongRef | undefined> => {
		const kept = published?.mixes?.find((mix) => mix.url === url)?.recording;

		if (kept) return isManagersTrack(kept, managers) ? kept : undefined;
		const id = plyrTrackId(url);
		const found = id && plyr ? await plyr.record(id) : null;

		return found && isManagersTrack(found, managers) ? found : undefined;
	};

	const mixes = await Promise.all(
		profile.mixes.map(async ({ recording: _ignored, ...mix }): Promise<ProjectMix> => {
			const recording = await recordingOf(mix.url);

			return recording ? { ...mix, recording } : mix;
		})
	);

	return { ...profile, mixes };
}
