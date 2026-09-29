"""Create time-ordered, pseudonymized training inputs from raw-capture JSONL.

Raw JSONL remains the archival source of truth. The derived feature table does
not use case labels, AI opinions, scores, or actions as training targets.
"""
from __future__ import annotations
import argparse
import csv
import datetime as dt
import hashlib
import hmac
import json
import math
import re
import secrets
from collections import Counter, defaultdict
from pathlib import Path
from typing import Any, Iterable

NUMERIC_BASE = [
    "cash_balance", "cash_change_{step}s", "kills_change_{step}s", "deaths_change_{step}s",
    "kpm_180_history_mean", "kpm_180_window_mean", "headshot_rate_window",
    "penetration_rate_window", "max_kills_15s_max", "unique_victims_window_max",
    "burst_points_window_max", "median_kill_interval_s", "roster_size", "active_fraction",
]
MASKS = [
    "cash_observed", "cash_change_valid", "combat_change_valid", "history_kpm_observed",
    "window_kpm_observed", "headshot_rate_observed", "penetration_rate_observed",
    "max15_observed", "unique_victims_observed", "burst_observed", "interval_observed",
    "roster_observed", "active_observed",
]
BUCKET_SECONDS = 60


def read_jsonl(path: Path) -> Iterable[dict[str, Any]]:
    with path.open(encoding="utf-8-sig") as f:
        for line_no, line in enumerate(f, 1):
            if line.strip():
                value = json.loads(line)
                if not isinstance(value, dict):
                    raise ValueError(f"Expected object in {path.name}:{line_no}")
                yield value


def parse_time(value: Any) -> dt.datetime | None:
    if not isinstance(value, str) or not value:
        return None
    result = dt.datetime.fromisoformat(value.replace("Z", "+00:00"))
    if result.tzinfo is None:
        raise ValueError(f"Timestamp lacks UTC offset: {value}")
    return result.astimezone(dt.timezone.utc)


def number(value: Any) -> float | None:
    if value is None or value == "":
        return None
    try:
        result = float(value)
    except (TypeError, ValueError):
        return None
    return result if math.isfinite(result) else None


def integer(value: Any) -> int | None:
    result = number(value)
    return int(result) if result is not None else None


def write_csv(path: Path, fields: list[str], rows: Iterable[dict[str, Any]]) -> int:
    count = 0
    with path.open("w", encoding="utf-8", newline="") as f:
        writer = csv.DictWriter(f, fieldnames=fields, extrasaction="ignore")
        writer.writeheader()
        for row in rows:
            writer.writerow(row)
            count += 1
    return count


def player_key(secret: bytes, server: str, steam_id: str | None) -> str:
    if not steam_id:
        return ""
    digest = hmac.new(secret, f"{server}|{steam_id}".encode(), hashlib.sha256).hexdigest()[:20]
    return "p_" + digest


def round_id(value: Any) -> str | None:
    match = re.search(r":match:(\d+)$", str(value or ""))
    return match.group(1) if match else None


def numeric_channels(bucket_seconds: int) -> list[str]:
    return [name.format(step=bucket_seconds) for name in NUMERIC_BASE]


def build(export_dir: Path, output_dir: Path, bucket_seconds: int = BUCKET_SECONDS) -> dict[str, Any]:
    if output_dir.exists():
        raise FileExistsError(f"Refusing to overwrite: {output_dir}")
    if bucket_seconds <= 0:
        raise ValueError("bucket_seconds must be positive")
    numeric = numeric_channels(bucket_seconds)
    cash_change_field = f"cash_change_{bucket_seconds}s"
    kills_change_field = f"kills_change_{bucket_seconds}s"
    deaths_change_field = f"deaths_change_{bucket_seconds}s"
    related = export_dir / "related_tables"
    matches: dict[str, dict[str, Any]] = {}
    for row in read_jsonl(related / "matches.jsonl"):
        mid = str(row["id"])
        start, end = parse_time(row.get("started_at")), parse_time(row.get("ended_at"))
        if start:
            matches[mid] = {"id": mid, "start": start, "end": end, "map": row.get("map") or ""}
    ordered_matches = sorted(matches.values(), key=lambda m: (m["start"], m["id"]))
    secret = secrets.token_bytes(32)

    progress: dict[tuple[str, str, str], list[dict[str, Any]]] = defaultdict(list)
    history: dict[tuple[str, str, str], list[dict[str, Any]]] = defaultdict(list)
    windows: dict[tuple[str, str, str], list[dict[str, Any]]] = defaultdict(list)
    source_counts: Counter[str] = Counter()
    rejects: Counter[str] = Counter()

    def add_progress(server: str, mid: str, steam: str | None, at: dt.datetime,
                     p: dict[str, Any], roster: int | None, source: str) -> None:
        match = matches.get(mid)
        if not match or not steam:
            rejects[source + "_invalid_key"] += 1
            return
        elapsed = (at - match["start"]).total_seconds()
        if elapsed < -5:
            rejects[source + "_before_match"] += 1
            return
        if match["end"] and elapsed >= (match["end"] - match["start"]).total_seconds():
            rejects[source + "_outside_match"] += 1
            return
        progress[(server, mid, str(steam))].append({
            "elapsed": max(0.0, elapsed), "at": at, "cash": number(p.get("cash")),
            "kills": integer(p.get("kills")), "deaths": integer(p.get("deaths")),
            "roster": roster, "source": source,
        })
        source_counts[source + "_player_observations"] += 1

    for row in read_jsonl(related / "player_progress_samples.jsonl"):
        at = parse_time(row.get("observed_at"))
        if not at:
            rejects["progress_missing_time"] += 1
            continue
        players = row.get("players") if isinstance(row.get("players"), list) else []
        for p in players:
            if isinstance(p, dict):
                add_progress(str(row.get("server_id") or ""), str(row.get("match_id") or ""),
                             p.get("steamId"), at, p, row.get("roster_size", len(players)), "progress_table")

    # Keep the captured full /v1/players payload useful for feature rebuilding.
    # It is collapsed with progress-table samples into one actual latest sample
    # per player/minute; missing keys in that selected payload remain missing.
    for row in read_jsonl(export_dir / "training_observations.jsonl"):
        if row.get("endpoint") != "/v1/players":
            continue
        at = parse_time(row.get("poll_started_at"))
        if not at:
            rejects["raw_players_missing_time"] += 1
            continue
        match = next((m for m in reversed(ordered_matches)
                      if m["start"] <= at and (m["end"] is None or at < m["end"])), None)
        if not match:
            rejects["raw_players_unmatched_round"] += 1
            continue
        payload = row.get("payload") if isinstance(row.get("payload"), dict) else {}
        players = payload.get("players") if isinstance(payload.get("players"), list) else []
        roster = integer(payload.get("count")) or len(players)
        for p in players:
            if isinstance(p, dict):
                add_progress(str(row.get("server_id") or ""), match["id"], p.get("steamId"),
                             at, p, roster, "raw_players_poll")

    for row in read_jsonl(related / "integrity_player_metric_history.jsonl"):
        mid, at, steam = round_id(row.get("round_id")), parse_time(row.get("observed_at")), row.get("steam_id")
        match = matches.get(mid or "")
        if not mid or not at or not match or not steam:
            rejects["history_invalid_key"] += 1
            continue
        elapsed = (at - match["start"]).total_seconds()
        if elapsed < -5 or (match["end"] and elapsed >= (match["end"] - match["start"]).total_seconds()):
            rejects["history_outside_match"] += 1
            continue
        history[(str(row.get("server_id") or ""), mid, str(steam))].append({
            "elapsed": max(0.0, elapsed), "at": at, "kpm": number(row.get("kpm_180")),
            "headrate": number(row.get("headshot_rate")), "max15": number(row.get("max_kills_15s")),
        })
        source_counts["metric_history_rows"] += 1

    for row in read_jsonl(related / "integrity_windows.jsonl"):
        mid, at, steam = round_id(row.get("round_id")), parse_time(row.get("observed_at")), row.get("steam_id")
        match = matches.get(mid or "")
        if not mid or not at or not match or not steam:
            rejects["window_invalid_key"] += 1
            continue
        elapsed = (at - match["start"]).total_seconds()
        if elapsed < -5 or (match["end"] and elapsed >= (match["end"] - match["start"]).total_seconds()):
            rejects["window_outside_match"] += 1
            continue
        windows[(str(row.get("server_id") or ""), mid, str(steam))].append({
            "elapsed": max(0.0, elapsed), "at": at, "inf": number(row.get("infantry_kills")),
            "kpm": number(row.get("kpm_180")), "victims": number(row.get("unique_victims")),
            "head": number(row.get("headshots")), "pen": number(row.get("penetrations")),
            "burst": number(row.get("burst_points")), "max15": number(row.get("max_kills_15s")),
            "interval": number(row.get("median_kill_interval")),
        })
        source_counts["integrity_window_rows"] += 1

    # Compare raw Feed identities to the normalized kills table, but do not add
    # both copies to features. The normalized table contains the longer history.
    raw_ids: set[tuple[str, str, str]] = set()
    raw_tags: dict[tuple[str, str, str], list[str] | None] = {}
    raw_types: Counter[str] = Counter()
    raw_count = raw_without_id = 0
    for batch in read_jsonl(export_dir / "training_feed_batches.jsonl"):
        payload = batch.get("payload") if isinstance(batch.get("payload"), dict) else {}
        for e in payload.get("events", []) if isinstance(payload.get("events"), list) else []:
            if not isinstance(e, dict):
                continue
            raw_count += 1
            raw_types[str(e.get("type") or "unknown")] += 1
            if e.get("eventId"):
                event_key = (str(batch.get("server_id") or ""), str(batch.get("instance_id") or ""),
                             str(e["eventId"]))
                raw_ids.add(event_key)
                context_tags = e.get("contextTags")
                raw_tags[event_key] = ([str(t) for t in context_tags if isinstance(t, str)]
                                       if isinstance(context_tags, list) else None)
            else:
                raw_without_id += 1

    kills: list[dict[str, Any]] = []
    seen: dict[tuple[str, str, str], str] = {}
    conflicts: set[tuple[str, str, str]] = set()
    exact_duplicates = 0
    for row in read_jsonl(related / "kills.jsonl"):
        key = (str(row.get("server_id") or ""), str(row.get("instance_id") or ""),
               str(row.get("event_id") or ""))
        canonical = json.dumps(row, sort_keys=True, separators=(",", ":"), ensure_ascii=False)
        if key[2] and key in seen:
            if seen[key] == canonical:
                exact_duplicates += 1
                continue
            conflicts.add(key)
        elif key[2]:
            seen[key] = canonical
        kills.append(row)
    raw_overlap = len(raw_ids.intersection(seen))

    # Use receive time only to recognize clock epochs. Within an epoch the
    # model sees only the source game clock, never receive time as event time.
    by_round: dict[tuple[str, str, str], list[dict[str, Any]]] = defaultdict(list)
    for row in kills:
        mid = str(row.get("match_row") or "")
        if mid not in matches:
            rejects["kill_unmatched_round"] += 1
            continue
        by_round[(mid, str(row.get("instance_id") or ""), str(row.get("match_id") or ""))].append(row)
    event_rows: list[dict[str, Any]] = []
    rollovers = 0
    for (mid, instance, source_match), events in by_round.items():
        events.sort(key=lambda e: (parse_time(e.get("ts")) or dt.datetime.min.replace(tzinfo=dt.timezone.utc),
                                   number(e.get("event_time")) if number(e.get("event_time")) is not None else -1.0))
        segment, previous = 0, None
        for row in events:
            clock = number(row.get("event_time"))
            if clock is not None and previous is not None and clock < previous - 300:
                segment += 1
                rollovers += 1
            if clock is not None:
                previous = clock
            tags = row.get("tags")
            tags_observed = isinstance(tags, list)
            distance_m = number(row.get("distance_m"))
            distance_valid = (not bool(row.get("distance_invalid")) and distance_m is not None
                              and 0 < distance_m < 5000)
            eid = str(row.get("event_id") or "")
            key = (str(row.get("server_id") or ""), instance, eid)
            raw_context = raw_tags.get(key, "not-captured")
            if raw_context != "not-captured":
                raw_context_observed = isinstance(raw_context, list)
                context_values = raw_context if raw_context_observed else []
                headshot = (int(any(str(t).lower().endswith("headshot") for t in context_values))
                            if raw_context_observed else None)
                penetration = (int(any(str(t).lower().endswith("penetration") for t in context_values))
                               if raw_context_observed else None)
                penetration_observed = raw_context_observed
                tags_value = ";".join(context_values) if raw_context_observed else ""
                headshot_observed = int(raw_context_observed)
                headshot_source = "raw_contextTags" if raw_context_observed else "raw_contextTags_missing"
                penetration_source = "raw_contextTags" if raw_context_observed else "raw_contextTags_missing"
            else:
                # Older normalized kills retain the parser's explicit boolean
                # and short non-headshot tags, but not the complete contextTags.
                headshot = int(bool(row.get("headshot"))) if row.get("headshot") is not None else None
                headshot_observed = int(row.get("headshot") is not None)
                headshot_source = "kills.headshot_parsed" if headshot_observed else "unknown"
                has_penetration = tags_observed and any(str(t).lower() == "penetration" for t in tags)
                # The normalized short-tag column cannot distinguish an
                # originally absent contextTags array from an empty one.
                penetration = 1 if has_penetration else None
                penetration_observed = bool(has_penetration)
                penetration_source = "kills.tags_positive" if has_penetration else "contextTags_presence_unavailable"
                tags_value = ";".join(str(t) for t in tags) if tags_observed else ""
            observed_at = parse_time(row.get("ts"))
            event_rows.append({
                "received_at_utc": observed_at.isoformat() if observed_at else "",
                "match_id": mid, "map": matches[mid]["map"], "clock_segment": segment,
                "event_time_seconds": clock,
                "killer_player_key": player_key(secret, str(row.get("server_id") or ""), row.get("killer_steam_id")),
                "victim_player_key": player_key(secret, str(row.get("server_id") or ""), row.get("victim_steam_id")),
                "weapon_id": str(row.get("cause") or "Unknown"),
                "raw_distance_cm": number(row.get("raw_distance_cm")),
                "distance_m": distance_m if distance_valid else None, "distance_valid": int(distance_valid),
                "headshot": headshot, "headshot_observed": headshot_observed,
                "penetration": penetration, "penetration_observed": int(penetration_observed),
                "headshot_source": headshot_source, "penetration_source": penetration_source,
                "tags": tags_value,
                "suicide": int(bool(row.get("suicide"))), "team_kill": int(bool(row.get("team_kill"))),
                "event_id_sha256_16": hashlib.sha256(eid.encode()).hexdigest()[:16] if eid else "",
                "event_id_observed": int(bool(eid)), "event_id_conflict": int(key in conflicts),
                "instance_id": instance, "source_match_id": source_match,
            })
    event_rows.sort(key=lambda e: (
        matches[e["match_id"]]["start"], e["match_id"], e["clock_segment"],
        e["event_time_seconds"] if e["event_time_seconds"] is not None else math.inf,
        e["received_at_utc"], e["killer_player_key"],
    ))

    # Convert observed state and statistics into chronological player-minute rows.
    pairs = set(progress) | set(history) | set(windows)
    all_rows: list[dict[str, Any]] = []
    sequences: list[dict[str, Any]] = []
    for key in sorted(pairs, key=lambda k: (matches[k[1]]["start"], k[0], k[2])):
        server, mid, steam = key
        match = matches[mid]
        pr = sorted(progress.get(key, []), key=lambda x: (x["elapsed"], x["source"] == "raw_players_poll"))
        hr, wr = history.get(key, []), windows.get(key, [])
        if not (pr or hr or wr):
            continue
        pm: dict[int, list[dict[str, Any]]] = defaultdict(list)
        hm: dict[int, list[dict[str, Any]]] = defaultdict(list)
        wm: dict[int, list[dict[str, Any]]] = defaultdict(list)
        for x in pr: pm[int(x["elapsed"] // bucket_seconds)].append(x)
        for x in hr: hm[int(x["elapsed"] // bucket_seconds)].append(x)
        for x in wr: wm[int(x["elapsed"] // bucket_seconds)].append(x)

        # The session table was not exported. Infer activity only between nearby
        # player-presence snapshots, with a 30-second tail for the last snapshot.
        spans: list[tuple[float, float]] = []
        duration = (match["end"] - match["start"]).total_seconds() if match["end"] else math.inf
        for i, x in enumerate(pr):
            t = x["elapsed"]
            nxt = pr[i + 1]["elapsed"] if i + 1 < len(pr) else t + min(bucket_seconds, 30.0)
            right = nxt if 0 < nxt - t <= 90 else t + 30.0
            spans.append((t, max(t, min(right, duration))))
        spans.extend((x["elapsed"], x["elapsed"] + 1.0) for x in hr + wr)
        spans = [(max(0.0, a), max(0.0, b)) for a, b in spans if b >= a]
        spans.sort()
        merged: list[tuple[float, float]] = []
        for a, b in spans:
            if merged and a <= merged[-1][1]:
                merged[-1] = (merged[-1][0], max(merged[-1][1], b))
            else:
                merged.append((a, b))
        if not merged:
            continue
        lo = min(int(a // bucket_seconds) for a, _ in merged)
        hi = max(int(max(a, b - 1e-9) // bucket_seconds) for _, b in merged)
        if match["end"]:
            # Never let a partial terminal minute cross the next match boundary.
            hi = min(hi, int(duration // bucket_seconds) - 1)
        if hi < lo:
            continue
        out: list[dict[str, Any]] = []
        previous: dict[str, Any] | None = None
        for minute in range(max(0, lo), hi + 1):
            left, right = minute * bucket_seconds, (minute + 1) * bucket_seconds
            ps = sorted(pm.get(minute, []), key=lambda x: (x["elapsed"], x["source"] == "raw_players_poll"))
            hs, ws = hm.get(minute, []), wm.get(minute, [])
            p = ps[-1] if ps else None
            active = min(float(bucket_seconds), sum(max(0.0, min(right, b, duration) - max(left, a))
                                   for a, b in merged))
            observed = bool(ps or hs or ws)
            if observed and active <= 0: active = 1.0
            row: dict[str, Any] = {
                "server_id": server, "match_id": mid, "map": match["map"],
                "player_key": player_key(secret, server, steam), "minute_index": minute,
                "seconds_from_match_start": left,
                "bucket_start_utc": (match["start"] + dt.timedelta(seconds=left)).isoformat(),
                "active_seconds": round(active, 3), "active_fraction": round(active / bucket_seconds, 6),
                "is_active": int(active > 0 or observed), "bucket_observed": int(observed),
                "progress_samples_in_bucket": len(ps), "metric_history_samples_in_bucket": len(hs),
                "integrity_windows_in_bucket": len(ws),
                "data_sources": ";".join(k for k, yes in (
                    ("player_state", bool(ps)), ("metric_history", bool(hs)), ("integrity_windows", bool(ws))
                ) if yes),
                "cash_balance": p["cash"] if p else None,
                "cash_observed": int(p is not None and p["cash"] is not None),
                "kills_cumulative": p["kills"] if p else None, "deaths_cumulative": p["deaths"] if p else None,
                "combat_counters_observed": int(p is not None and p["kills"] is not None and p["deaths"] is not None),
                "roster_size": p["roster"] if p else None,
                "roster_observed": int(p is not None and p["roster"] is not None),
                cash_change_field: None, "cash_change_valid": 0,
                kills_change_field: None, deaths_change_field: None, "combat_change_valid": 0,
                "counter_reset": 0, "observation_gap_seconds": None,
            }
            if previous and minute == previous["minute"] + 1 and p and previous["p"]:
                gap = p["elapsed"] - previous["p"]["elapsed"]
                row["observation_gap_seconds"] = round(gap, 3)
                if 0 <= gap <= bucket_seconds * 1.5:
                    if p["cash"] is not None and previous["p"]["cash"] is not None:
                        row[cash_change_field] = p["cash"] - previous["p"]["cash"]
                        row["cash_change_valid"] = 1
                    vals = (p["kills"], previous["p"]["kills"], p["deaths"], previous["p"]["deaths"])
                    if all(v is not None for v in vals):
                        dk, dd = vals[0] - vals[1], vals[2] - vals[3]
                        if dk >= 0 and dd >= 0:
                            row[kills_change_field], row[deaths_change_field] = dk, dd
                            row["combat_change_valid"] = 1
                        else:
                            row["counter_reset"] = 1

            hk = [x["kpm"] for x in hs if x["kpm"] is not None]
            wk = [x["kpm"] for x in ws if x["kpm"] is not None]
            row["kpm_180_history_mean"] = sum(hk) / len(hk) if hk else None
            row["history_kpm_observed"] = int(bool(hk))
            row["kpm_180_window_mean"] = sum(wk) / len(wk) if wk else None
            row["window_kpm_observed"] = int(bool(wk))
            inf = [x["inf"] for x in ws if x["inf"] is not None and x["inf"] > 0]
            heads = [x["head"] for x in ws if x["head"] is not None]
            pens = [x["pen"] for x in ws if x["pen"] is not None]
            row["headshot_rate_window"] = sum(heads) / sum(inf) if inf and heads and sum(inf) > 0 else None
            row["penetration_rate_window"] = sum(pens) / sum(inf) if inf and pens and sum(inf) > 0 else None
            for name in ("headshot_rate_window", "penetration_rate_window"):
                if row[name] is not None and not 0 <= row[name] <= 1: row[name] = None
            row["headshot_rate_observed"] = int(row["headshot_rate_window"] is not None)
            row["penetration_rate_observed"] = int(row["penetration_rate_window"] is not None)
            for target, source, mask in (
                ("max_kills_15s_max", "max15", "max15_observed"),
                ("unique_victims_window_max", "victims", "unique_victims_observed"),
                ("burst_points_window_max", "burst", "burst_observed"),
            ):
                values = [x[source] for x in ws if x[source] is not None]
                row[target] = max(values) if values else None
                row[mask] = int(bool(values))
            intervals = [x["interval"] for x in ws if x["interval"] is not None]
            row["median_kill_interval_s"] = intervals[-1] if intervals else None
            row["interval_observed"] = int(bool(intervals))
            row["active_observed"] = int(active > 0)
            row["quality_flags"] = ";".join(x for x, yes in (
                ("activity_inferred_no_sessions_table", True), ("counter_reset", bool(row["counter_reset"])),
                ("progress_missing", not bool(ps)),
            ) if yes)
            out.append(row)
            previous = {"minute": minute, "p": p}

        all_rows.extend(out)
        n = max(1, len(out))
        sequences.append({
            "match_id": mid, "map": match["map"], "player_key": player_key(secret, server, steam),
            "start_utc": out[0]["bucket_start_utc"], "end_utc": out[-1]["bucket_start_utc"],
            "minutes": round(len(out) * bucket_seconds / 60, 3), "time_buckets": len(out),
            "duration_seconds": len(out) * bucket_seconds,
            "observed_bucket_count": sum(x["bucket_observed"] for x in out),
            "observed_pct": round(sum(x["bucket_observed"] for x in out) / n, 4),
            "cash_observed_pct": round(sum(x["cash_observed"] for x in out) / n, 4),
            "combat_counter_coverage_pct": round(sum(x["combat_counters_observed"] for x in out) / n, 4),
            "progress_observations": len(pr), "history_rows": len(hr), "window_rows": len(wr),
        })

    all_rows.sort(key=lambda r: (matches[r["match_id"]]["start"], r["player_key"], r["minute_index"]))
    sequences.sort(key=lambda r: (matches[r["match_id"]]["start"], r["player_key"]))
    output_dir.mkdir(parents=True)
    bucket_fields = [
        "server_id", "match_id", "map", "player_key", "minute_index", "seconds_from_match_start",
        "bucket_start_utc", "active_seconds", "active_fraction", "is_active", "bucket_observed",
        "progress_samples_in_bucket", "metric_history_samples_in_bucket", "integrity_windows_in_bucket",
        "data_sources", *numeric[:1], "cash_observed", cash_change_field, "cash_change_valid",
        "kills_cumulative", "deaths_cumulative", "combat_counters_observed", kills_change_field,
        deaths_change_field, "combat_change_valid", "kpm_180_history_mean", "history_kpm_observed",
        "kpm_180_window_mean", "window_kpm_observed", "headshot_rate_window", "headshot_rate_observed",
        "penetration_rate_window", "penetration_rate_observed", "max_kills_15s_max", "max15_observed",
        "unique_victims_window_max", "unique_victims_observed", "burst_points_window_max", "burst_observed",
        "median_kill_interval_s", "interval_observed", "roster_size", "roster_observed", "counter_reset",
        "observation_gap_seconds", "quality_flags",
    ]
    n_buckets = write_csv(output_dir / "complete_time_buckets.csv", bucket_fields, all_rows)
    seq_fields = list(sequences[0]) if sequences else ["match_id", "map", "player_key", "start_utc", "end_utc"]
    n_sequences = write_csv(output_dir / "sequence_index.csv", seq_fields, sequences)
    event_fields = [
        "received_at_utc", "match_id", "map", "clock_segment", "event_time_seconds",
        "killer_player_key", "victim_player_key", "weapon_id", "raw_distance_cm", "distance_m",
        "distance_valid", "headshot", "headshot_observed", "penetration", "penetration_observed",
        "headshot_source", "penetration_source", "tags", "suicide", "team_kill",
        "event_id_sha256_16", "event_id_observed", "event_id_conflict",
        "instance_id", "source_match_id",
    ]
    n_events = write_csv(output_dir / "kills_chronological.csv", event_fields, event_rows)
    coverage = Counter(r["match_id"] for r in sequences)
    report = {
        "schema_version": "raw-capture-chronological-v1",
        "bucket_seconds": bucket_seconds,
        "source_export": str(export_dir.resolve()),
        "match_rows": len(matches),
        "matches": [{"match_id": m["id"], "map": m["map"], "start_utc": m["start"].isoformat(),
                     "end_utc": m["end"].isoformat() if m["end"] else None} for m in ordered_matches],
        "derived_files": {"complete_time_buckets.csv": {"rows": n_buckets},
                          "sequence_index.csv": {"rows": n_sequences},
                          "kills_chronological.csv": {"rows": n_events}},
        "input_rows": {
            "progress_table_player_observations": source_counts["progress_table_player_observations"],
            "raw_players_poll_player_observations": source_counts["raw_players_poll_player_observations"],
            "metric_history": source_counts["metric_history_rows"], "integrity_windows": source_counts["integrity_window_rows"],
            "normalized_kills_after_dedup": len(kills), "raw_feed_events": raw_count,
            "raw_feed_event_types": dict(raw_types), "raw_feed_ids_overlapping_normalized_kills": raw_overlap,
        },
        "kill_event_quality": {
            "exact_duplicate_ids_removed": exact_duplicates, "conflicting_event_ids": len(conflicts),
            "clock_rollovers_detected": rollovers, "unmatched_kill_rounds": rejects["kill_unmatched_round"],
            "raw_feed_events_without_id_preserved": raw_without_id,
        },
        "player_match_sequences": n_sequences,
        "sequences_by_match": dict(sorted(coverage.items(), key=lambda x: matches[x[0]]["start"])),
        "source_rejects": dict(rejects),
        "time_semantics": {
            "state_and_statistics": f"UTC observed_at/poll_started_at aligned to matches.started_at in {bucket_seconds}-second buckets",
            "kill_event_time": "event_time_seconds is the game-relative source clock, segmented on rollbacks; never Unix time",
            "received_at": "retained for audit and approximate clock-epoch ordering only, not treated as exact kill time",
            "activity": "inferred from nearby player-presence snapshots because player_sessions was not exported",
        },
        "features": {
            "channel_order": numeric + MASKS,
            "weapon_distance": "not in the 27 channels; raw cause and valid source distance are available to Event Transformer",
            "missingness": "absent player values remain null; deltas require adjacent valid samples",
            "historical_event_tags": "raw Feed tags are exact where archived; older normalized kills preserve parsed headshot, but an absent short Penetration tag cannot prove the original contextTags array was complete",
            "labels_used": False, "AI_opinions_or_scores_used_as_truth": False,
        },
        "pseudonymization": "Derived files use per-run HMAC player keys; random key not saved. Raw JSONL unchanged.",
    }
    (output_dir / "source_quality.json").write_text(
        json.dumps(report, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    print(json.dumps({"buckets": n_buckets, "sequences": n_sequences, "kill_rows": n_events,
                      "matches": len(matches), "matches_with_state": len(coverage),
                      "raw_feed_overlap": raw_overlap, "clock_rollovers": rollovers,
                      "output": str(output_dir)}, ensure_ascii=False))
    return report


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--export-dir", type=Path, required=True)
    parser.add_argument("--output-dir", type=Path, required=True)
    parser.add_argument("--bucket-seconds", type=int, default=BUCKET_SECONDS)
    args = parser.parse_args()
    build(args.export_dir.resolve(), args.output_dir.resolve(), args.bucket_seconds)
