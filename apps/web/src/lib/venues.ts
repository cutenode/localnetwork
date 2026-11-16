/**
 * A venue as the venue picker offers it. `space` is what a form sends for it, `skey` what its page goes by, and its
 * city tells it apart from others of the same name.
 */
export type VenueOption = { space: string; skey: string; name: string; locality?: string; country: string };

/** The body `/api/venues` answers a search with. */
export type VenuesResponse = { venues: VenueOption[] };

/** The body `/api/venues` answers an add with: the venue added, or the one that was already there. */
export type AddVenueResponse = { venue: VenueOption } | { error: string };
