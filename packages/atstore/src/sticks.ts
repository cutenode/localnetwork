// Blank USB sticks for locallibrary's fixture drives: each a FAT32 disk image a stick's size,
// mounted under a folder of the run's own, so locallibrary sees volumes mounted at their own roots
// with a stick's space, rather than folders on this Mac's disk. Mounted with `nobrowse`, out of
// /Volumes and the Finder. macOS only (hdiutil and diskutil).
import { mkdirSync } from 'node:fs';
import { join } from 'node:path';
import { run } from './run.ts';

/**
 * A 32 GB stick's size, 32,000,000,000 bytes, in 512-byte sectors (hdiutil's `g` is GiB). The
 * images are sparse, so they take only what's written to them.
 */
const SIZE = '62500000b';

/** Sticks that are mounted, and how to take them out again. */
export interface Sticks {
	/** The folder they're mounted in, `<dir>/Volumes`, each at its name. */
	volumes: string;
	/** Ejects every one of them, trying them all before failing over any that wouldn't go. */
	eject(): Promise<void>;
}

/** Makes a stick for each of `names` (a FAT32 volume name: 11 letters at most) in `dir`, and mounts it. */
export async function mountSticks(dir: string, names: string[]): Promise<Sticks> {
	const volumes = join(dir, 'Volumes');
	const mounted: string[] = [];

	const eject = async () => {
		const failed: Error[] = [];

		for (const mountPoint of mounted.splice(0).toReversed()) {
			// A process with a file open on it keeps diskutil from ejecting it, and the run is over:
			// then it's forced off.
			await run('diskutil', ['eject', mountPoint])
				.catch(() => run('hdiutil', ['detach', '-force', mountPoint]))
				.catch((error: Error) => failed.push(error));
		}

		if (failed.length > 0) throw new AggregateError(failed, `${failed.length} stick(s) wouldn't eject`);
	};

	try {
		for (const name of names) {
			const image = join(dir, name);
			const mountPoint = join(volumes, name);
			await run('hdiutil', [
				'create',
				'-quiet',
				'-size',
				SIZE,
				'-type',
				'SPARSE',
				'-fs',
				'MS-DOS FAT32',
				'-volname',
				name,
				image
			]);
			mkdirSync(mountPoint, { recursive: true });
			await run('diskutil', [
				'image',
				'attach',
				'--mountOptions',
				'nobrowse',
				'--mountPoint',
				mountPoint,
				`${image}.sparseimage`
			]);
			mounted.push(mountPoint);
		}
	} catch (error) {
		// The sticks made so far go too, and the error that stopped them is the one that's thrown.
		await eject().catch((ejecting: Error) => console.error(ejecting));
		throw error;
	}

	return { volumes, eject };
}
