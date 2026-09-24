# WARDOGS Community Integrity System

This is NOT a client anti-cheat. It does not inspect player devices. It is a server-side behavioral integrity and moderation system.

The work is being added to Warcon in reviewable phases. **No automated integrity kick or quarantine is active yet.** The [technical implementation plan](wardogs-community-integrity-plan.zh-CN.md) records the planned risk scoring, evidence, reports, appeals, Discord integration, privacy policy, false-positive handling, administrator workflow and rollout gates. Existing Warcon automation rules continue to operate independently.

## Current identity model (Phase 1)

`integrity_profiles` has one row per organisation and SteamID64. It stores the latest observed name, distinct names seen and first/last observation times. It is backfilled from `player_sessions`; subsequent joins, name changes and session heartbeats update it in the worker's fenced observation transaction. Sessions, match statistics and kills remain the source of truth for playtime and combat metrics. Names are display data, never an identity key.

This profile does not collect device IDs, local files, browsing data or IP-derived player profiles.

## Weapon classification (Phase 2)

`src/lib/server/integrity/weapons.ts` classifies only explicitly observed small-arm cause tags as `INFANTRY`. Unmapped causes remain `UNKNOWN` and do not count toward infantry KPM. Road kills, vehicle explosions, falls and suicides take precedence over any cause mapping. A kill is eligible for infantry rate calculations only when both SteamID64s and distinct factions are known.

Organisation owners can list, set and remove exact cause overrides through `GET`/`PUT`/`DELETE /api/orgs/:id/integrity/weapons`. Changes are audited; removing an override returns the cause to its built-in classification. The configuration UI will follow with the Integrity dashboard. The classifier is not yet connected to automated actions.

## Infantry KPM (Phase 3)

The worker now observes accepted Kill Feed events in an in-memory, rolling 180-second game-clock window for each SteamID64 and server. It counts only classified infantry kills with two known, opposing factions. By default an abnormal window starts at 12 valid kills (KPM 4.0); another finding cannot be recorded until 180 game-clock seconds later. Map changes, instance changes and substantial clock rewinds clear the window. Distinct abnormal findings are stored in `integrity_windows` with their event IDs and unique victim count. The game-clock data is not sufficient to claim an aim cheat, and these findings **do not kick or ban players**. Evidence confidence is a later phase.

## Explainable risk scoring (Phase 4)

`integrity_scores` stores each abnormal finding's 0–100 score, individual contributions and the organisation's rule version. `GET`/`PUT /api/orgs/:id/integrity/rules` lets an organisation owner inspect and update bounded weights and thresholds; changes are audited. Rule sets start in `dry_run`, and the API rejects `enforce` until the action, evidence and review phases are implemented. Current live scores include KPM, distinct victims and earlier independent windows. Report, Steam, burst and other inputs are defined by the pure scoring model but remain zero or UNKNOWN until their trusted data sources are connected. These scores are **advisory only** and are separate from Warcon's existing connect-time risk score.

## Evidence cases (Phase 5)

When an advisory score reaches the configured AUTO_KO threshold, the worker saves a `CASE-…` record in the same fenced transaction as the abnormal window and score. It freezes the underlying accepted kill events in the 180-second game-clock window, the exact trigger event IDs, rule configuration/version, risk breakdown and player/server identifiers. A complete stored trigger set receives confidence B; missing events or truncation lower it to C/D. Confidence A is reserved for a future verified continuous-feed check. A case is evidence for staff review, **not a finding of cheating or a game action**. Reviews, appeals, retention policy and action history remain later phases.

## Administrator dashboard (Phase 6)

The server's new Integrity tab lists recent cases, risk scores, rule version and latest kill-feed receipt. Its content requires the new `integrity.view` capability; existing built-in admin roles receive that read capability, while custom roles must be granted it by an organisation owner. The page has 简体中文 and English labels, and [README.zh-CN.md](../README.zh-CN.md) introduces the fork in Chinese. Automated actions remain off. Full case review and the rest of the panel's translations are still to be built.

## Protocol limit

Warcon has a verified outgoing player whisper and a server-to-panel kill feed, but no verified incoming game-chat event. In-game `!report` and `!BAN` commands cannot be enabled until an authorised chat source is available. `!BAN` will only create a report, never a ban, when such a source exists.
