// Blank USB sticks for locallibrary's fixture drives: each a FAT32 disk image a stick's size,
// mounted under a folder of the run's own, so locallibrary sees volumes mounted at their own roots
// with a stick's space, rather than folders on this Mac's disk. Mounted with `nobrowse`, out of
// /Volumes and the Finder. macOS only (hdiutil and diskutil).
import { existsSync, mkdirSync, readdirSync, statSync } from 'node:fs';
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
	/** Ejects every one of them, and says whether they all went. One that wouldn't is logged. */
	eject(): Promise<boolean>;
}

/**
 * Ejects each of `mountPoints`, and says whether they all went. A process with a file open on one
 * keeps diskutil from ejecting it, and the run is over by then: then it's forced off.
 */
async function ejectAll(mountPoints: string[]): Promise<boolean> {
	let ejected = true;

	for (const mountPoint of mountPoints) {
		await run('diskutil', ['eject', mountPoint])
			.catch(() => run('diskutil', ['eject', 'force', mountPoint]))
			.catch((error: Error) => {
				console.error(`${mountPoint} wouldn't eject: ${error.message}`);
				ejected = false;
			});
	}

	return ejected;
}

/**
 * Ejects the sticks a run that never got to eject them left mounted in `dir`, and says whether
 * they all went. A folder is a mount point when it's on another device than the folder it's in.
 */
export async function ejectLeftovers(dir: string): Promise<boolean> {
	const volumes = join(dir, 'Volumes');

	if (!existsSync(volumes)) return true;
	const { dev } = statSync(volumes);

	return ejectAll(
		readdirSync(volumes)
			.map((name) => join(volumes, name))
			.filter((path) => statSync(path).dev !== dev)
	);
}

/** Makes a stick for each of `names` (a FAT32 volume name: 11 letters at most) in `dir`, and mounts it. */
export async function mountSticks(dir: string, names: string[]): Promise<Sticks> {
	const volumes = join(dir, 'Volumes');
	const mounted: string[] = [];
	const eject = () => ejectAll(mounted.splice(0).toReversed());

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
		await eject();
		throw error;
	}

	return { volumes, eject };
}
