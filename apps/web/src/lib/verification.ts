import type { IssuedVerification, LiveIdentity } from './types';

/**
 * Whether a verification still holds, by the rule Bluesky's AppView applies: the account's handle and display name
 * are exactly what was verified, and the display name isn't empty. A rename or a new handle turns it off.
 */
export function isCurrent(issued: Pick<IssuedVerification, 'handle' | 'displayName'>, live: LiveIdentity): boolean {
	return !!live.displayName && live.displayName === issued.displayName && live.handle === issued.handle;
}
