// The shared shapes (types.d.ts) as the global namespace `Tbx`, for the classic scripts at the
// root (content.ts, downloaded-store.ts, state-transfer.ts). A script cannot import, and a file
// that did would be a module, whose top-level names stop being the globals the others reach.
export type * from './types';

/** A value as JSON carries it, which is all chrome.storage, a state file or SoundCloud's API holds. */
export type Json = null | boolean | number | string | Json[] | JsonObject;

/** A JSON object, read member by member. */
export type JsonObject = { [name: string]: Json };

export as namespace Tbx;
