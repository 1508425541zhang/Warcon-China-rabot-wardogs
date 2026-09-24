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

The worker now observes accepted Kill Feed events in an in-memory, rolling 180-second game-clock window for each SteamID64 and server. It counts only classified infantry kills with two known, opposing factions. An abnormal window starts at 12 valid kills (KPM 4.0); another finding cannot be recorded until 180 game-clock seconds later. Map changes, instance changes and substantial clock rewinds clear the window. Distinct abnormal findings are stored in `integrity_windows` with their event IDs and unique victim count. The game-clock data is not sufficient to claim an aim cheat, and these findings **do not kick or ban players**. Configurable thresholds, scoring and evidence confidence are later phases.

## Protocol limit

Warcon has a verified outgoing player whisper and a server-to-panel kill feed, but no verified incoming game-chat event. In-game `!report` and `!BAN` commands cannot be enabled until an authorised chat source is available. `!BAN` will only create a report, never a ban, when such a source exists.
