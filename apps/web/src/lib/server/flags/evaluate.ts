import { createHash } from 'node:crypto';

/** What decides a flag for one caller. locallibrary is told the same three things by getFlags. */
export type FlagGates = { everyone: boolean; targeted: boolean; percentage: number };

/** Where `did` sits in `key`'s rollout, 0 to 99. It never changes, so an account stays in as a percentage grows. */
export function bucket(key: string, did: string): number {
	return (
		createHash('sha256')
			.update(key + did, 'utf8')
			.digest()
			.readUInt32BE(0) % 100
	);
}

/** The rule flags/README.md states. flags/vectors.json holds every tool that decides a flag to it. */
export function isOn({ everyone, targeted, percentage }: FlagGates, key: string, did: string | null): boolean {
	return everyone || targeted || (did !== null && bucket(key, did) < percentage);
}
