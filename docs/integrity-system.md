# WARDOGS Community Integrity System

This is NOT a client anti-cheat. It does not inspect player devices. It is a server-side behavioral integrity and moderation system.

The work is being added to Warcon in reviewable phases. **No automated integrity kick or quarantine is active yet.** The [technical implementation plan](wardogs-community-integrity-plan.zh-CN.md) records the planned risk scoring, evidence, reports, appeals, Discord integration, privacy policy, false-positive handling, administrator workflow and rollout gates. Existing Warcon automation rules continue to operate independently.

## Current identity model (Phase 1)

`integrity_profiles` has one row per organisation and SteamID64. It stores the latest observed name, distinct names seen and first/last observation times. It is backfilled from `player_sessions`; subsequent joins, name changes and session heartbeats update it in the worker's fenced observation transaction. Sessions, match statistics and kills remain the source of truth for playtime and combat metrics. Names are display data, never an identity key.

This profile does not collect device IDs, local files, browsing data or IP-derived player profiles.

## Weapon classification (Phase 2)

`src/lib/server/integrity/weapons.ts` classifies only explicitly observed small-arm cause tags as `INFANTRY`. Unmapped causes remain `UNKNOWN` and do not count toward infantry KPM. Road kills, vehicle explosions, falls and suicides take precedence over any cause mapping. A kill is eligible for infantry rate calculations only when both SteamID64s and distinct factions are known.

Organisation owners can list, set and remove exact cause overrides through `GET`/`PUT`/`DELETE /api/orgs/:id/integrity/weapons` and the Integrity dashboard's **风控设置** section. Changes are audited; removing an override returns the cause to its built-in classification. The classifier is not yet connected to automated actions.

## Infantry KPM (Phase 3)

The worker observes accepted Kill Feed events in an in-memory, rolling 180-second game-clock window for each SteamID64 and server. It counts only classified infantry kills with two known, opposing factions. KPM, headshot share, penetration share or a game-clock burst can independently trigger a behavior finding. An active window has one persisted row; new reasons or stronger tiers update it and append a score snapshot. Identical tiers cause no write. These findings **do not kick or ban players**.

## Explainable risk scoring (Phase 4)

`integrity_scores` stores each meaningful behavior snapshot's 0–100 score, individual contributions and the organisation's rule version. `GET`/`PUT /api/orgs/:id/integrity/rules` lets an organisation owner inspect and update bounded weights and thresholds; changes are audited. Live scores use KPM, distinct victims, headshot and penetration shares, event-clock bursts, unique reporters, valid cached Steam VAC/game-ban history and a previous recorded KO level within the configured review period. Steam is queried only after a behavior finding and never inside the score transaction. Missing or stale Steam data is UNKNOWN and contributes nothing. Official WARDOGS playtime remains UNKNOWN. Rule sets stay in `dry_run`; the API rejects `enforce`. Scores are **advisory only** and separate from Warcon's existing connect-time risk score. KD has no scoring weight.

## Evidence cases (Phase 5)

When an advisory score reaches the configured AUTO_KO threshold, the worker saves a `CASE-…` record in the same fenced transaction as the abnormal window and score. It freezes the underlying accepted kill events in the 180-second game-clock window, the exact trigger event IDs, rule configuration/version, risk breakdown and player/server identifiers. A complete stored trigger set receives confidence B; missing events or truncation lower it to C/D. Confidence A is reserved for a future verified continuous-feed check. A case is evidence for staff review, **not a finding of cheating or a game action**. Reviews, appeals, retention policy and action history remain later phases.

## Administrator dashboard (Phase 6)

The server's Integrity tab lists recent cases, risk scores, rule version and latest kill-feed receipt. It now shows the current roster's kills, deaths, KD, valid 180-second infantry KPM, ten-minute peak, distinct victims and recorded Integrity score/level/breakdown. The player dossier shows the same Integrity fields plus aliases and recent anomaly details. Stale or truncated feed data hides KPM instead of presenting it as reliable. Its content requires `integrity.view`; only organisation owners see editable rules and weapon classification. The page has 简体中文 and English labels, and [README.zh-CN.md](../README.zh-CN.md) introduces the fork in Chinese. Setup, sign-in, registration and main navigation now have Chinese text. Other legacy panel pages are still largely English. Automated actions remain off.

## Community reports (Phase 7)

Players can use the bilingual `/s/:id/report` form when a server's public status page is enabled.

`POST /api/reports` accepts `{ "serverId": "...", "target": "SteamID64 or name", "reason": "..." }` from a signed-in user with a verified Steam account link. The target must have appeared on that server in the last 15 minutes. Exact SteamID64 and name matching take priority; ambiguous fuzzy names are rejected. The same reporter and target have a 10-minute cooldown, and one reporter may send at most five reports per hour. Only distinct reporter SteamID64s in the previous 24 hours contribute to risk; report-only scores have no behavior anomaly and cause no action. The next abnormal infantry window combines its evidence with that distinct reporter count. Each report copies up to 1000 accepted target-related kill events from T−180 seconds and subsequently captures events through T+180 seconds in `integrity_report_events`. Staff see recent reports on the Integrity dashboard without exposing reporter identities there. Reports and scores are audited and remain advisory.

The `!report` and `!BAN` command strings are parsed and tested, but no in-game command listener is active. Warcon currently has no verified inbound chat feed; this limitation is explicit rather than treating an outgoing whisper as incoming chat.

## Discord case alerts (Phase 9)

An organisation can opt an existing Discord webhook into the new **Community Integrity evidence cases** event class. A case created at or above the configured KO risk threshold sends a compact, rate-limited embed after its database transaction commits. It includes the Case ID, SteamID64, server, map, risk breakdown and 180-second infantry figures, and explicitly says that no Integrity kick or quarantine occurred in Dry Run. Reporter identities and ordinary single reports are never mirrored. Webhook failures do not change the stored case.

Phase 8 broadcast is held because advertising in-game report commands before an authenticated inbound chat source exists would mislead players. Warcon's existing scheduled broadcast facility remains available for other server notices.

## Dry Run impact preview (Phase 10)

The Integrity dashboard shows the past 24 hours, 72 hours and 7 days of recorded abnormal-window scores: number of windows and distinct players whose stored scores meet the current KO or quarantine thresholds with a current behavior anomaly. It also aggregates the top rule contributions over seven days. This is a threshold preview over scores produced under their recorded rule versions, not a full historical rule replay. It performs no kicks, quarantine or bans. Switching to enforcement remains unavailable while the action and false-positive controls are incomplete.

## Protocol limit

Warcon has a verified outgoing player whisper and a server-to-panel kill feed, but no verified incoming game-chat event. In-game `!report` and `!BAN` commands cannot be enabled until an authorised chat source is available. `!BAN` will only create a report, never a ban, when such a source exists.
