// What a KOTH match plays to. Live builds send no `scoreCap` on GET /v1/status and the config
// document has no key for it: the cap is a game constant, 100 by default. Everything that draws
// or mentions the cap reads it here, so a change in the game is one line.

import type { Status } from './types';

/** The score a faction plays to. Hard-coded in the game; the status carries it only on some builds. */
export const DEFAULT_SCORE_CAP = 100;

/** The cap the status reports, else the game's default. */
export const scoreCapOf = (status: Pick<Status, 'scoreCap'> | null | undefined): number =>
	status?.scoreCap || DEFAULT_SCORE_CAP;
