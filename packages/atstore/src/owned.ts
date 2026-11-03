// What atstore will use in a folder anyone can write to, such as /tmp: only what this user made.
import { lstatSync } from 'node:fs';

// SAFETY: atstore runs on macOS and Linux, where every process has a user id.
const ME = process.getuid!();

/**
 * Whether `path` is there, refusing it unless it's a `kind` of `uid`'s own and not a symlink:
 * someone else could have made it first, or pointed it somewhere else.
 */
export function ownedOrAbsent(path: string, kind: 'file' | 'directory', uid = ME): boolean {
	const stat = lstatSync(path, { throwIfNoEntry: false });

	if (!stat) return false;

	if (stat.isSymbolicLink()) throw new Error(`${path} is a symlink, which atstore won't follow: remove it`);

	if (kind === 'file' ? !stat.isFile() : !stat.isDirectory()) throw new Error(`${path} isn't a ${kind}: remove it`);

	if (stat.uid !== uid) throw new Error(`${path} belongs to user ${stat.uid}, not this one: remove it`);

	return true;
}
