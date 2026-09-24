# WARDOGS Community Integrity System

This is NOT a client anti-cheat. It does not inspect player devices. It is a server-side behavioral integrity and moderation system.

The work is being added to Warcon in reviewable phases. **No automated integrity kick or quarantine is active yet.** The [technical implementation plan](wardogs-community-integrity-plan.zh-CN.md) records the planned risk scoring, evidence, reports, appeals, Discord integration, privacy policy, false-positive handling, administrator workflow and rollout gates. Existing Warcon automation rules continue to operate independently.

## Current identity model (Phase 1)

`integrity_profiles` has one row per organisation and SteamID64. It stores the latest observed name, distinct names seen and first/last observation times. It is backfilled from `player_sessions`; subsequent joins, name changes and session heartbeats update it in the worker's fenced observation transaction. Sessions, match statistics and kills remain the source of truth for playtime and combat metrics. Names are display data, never an identity key.

This profile does not collect device IDs, local files, browsing data or IP-derived player profiles.

## Protocol limit

Warcon has a verified outgoing player whisper and a server-to-panel kill feed, but no verified incoming game-chat event. In-game `!report` and `!BAN` commands cannot be enabled until an authorised chat source is available. `!BAN` will only create a report, never a ban, when such a source exists.
