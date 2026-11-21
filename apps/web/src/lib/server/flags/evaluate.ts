import { createHash } from 'node:crypto';

/**
 * What decides a flag for one caller. locallibrary is told the same four things by getFlags. `enabled` false turns it
 * off for everyone while the other three stay stored, so turning it back on gives the same answers as before.
 */
export type FlagGates = { enabled: boolean; everyone: boolean; targeted: boolean; percentage: number };

/**
 * Where `id`, an account's DID or a signed-out visitor's id (see visitor.ts), sits in `key`'s rollout, 0 to 99. It never
 * changes, so an account or a browser stays in as a percentage grows.
 */
export function bucket(key: string, id: string): number {
	return (
		createHash('sha256')
			.update(key + id, 'utf8')
			.digest()
			.readUInt32BE(0) % 100
	);
}

/**
 * The rule flags/README.md states. flags/vectors.json holds every tool that decides a flag to it. `id` is null for a
 * caller with neither a DID nor a visitor id, who only gets what's on for everyone.
 */
export function isOn({ enabled, everyone, targeted, percentage }: FlagGates, key: string, id: string | null): boolean {
	return enabled && (everyone || targeted || (id !== null && bucket(key, id) < percentage));
}
