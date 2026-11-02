// A file of its own so dom.ts can be read without the chrome stand-in, as packages/atstore reads
// it to draw a playlist page for the website.

/** Plain JSON, which is all chrome.storage keeps: Chrome serializes every value it is given. */
export type Json = string | number | boolean | null | Json[] | { [key: string]: Json | undefined };
