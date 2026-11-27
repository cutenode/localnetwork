import type { create } from 'asciinema-player';

export type Player = ReturnType<typeof create>;

/** Each moving story's player, by the element it plays in, so a test can hold it on a frame. */
export const players = new WeakMap<HTMLElement, Player>();
