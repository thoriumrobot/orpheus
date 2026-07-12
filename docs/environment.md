# Environment variables

Every knob Orpheus reads from the environment, in one place. Nothing here is required —
the defaults are chosen so `latte` works out of the box — but each var lets you relocate
state, harden a deployment, or change engine behavior. `latte status` prints the ones that
matter for the current run.

## Engine

| variable | effect | default |
|----------|--------|---------|
| `ORPHEUS_FUEL` | interpreter step budget; `0` = unlimited. Raise it for very heavy pure-interpret work; the default already scales up automatically when no `rustc` is present. | auto (50M with a native compiler, 20G without) |
| `ORPHEUS_NO_RUSTC` | `1` skips the `rustc` probe entirely and forces interpret-only. The Android launcher sets this so startup never spawns a process. | unset (probe runs) |
| `ORPHEUS_OPT` | rustc optimization level for Anvil-compiled programs (`0`–`3`). Higher = slower builds, faster runs. | `0` |

## Compiled-program cache (Anvil)

| variable | effect | default |
|----------|--------|---------|
| `ORPHEUS_CACHE` | root for all caches (the native program cache, the market data, the news wire). | `~/.cache/orpheus` (Linux), `%LOCALAPPDATA%\orpheus` (Windows) |
| `ORPHEUS_CACHE_MAX` | cache size cap in MiB before LRU eviction; `0` = unbounded. | unbounded |
| `ORPHEUS_CACHE_SHARED` | a directory to share compiled builds across hosts with the same toolchain. | off |

## Data & the news wire

| variable | effect | default |
|----------|--------|---------|
| `ORPHEUS_NEWS` | directory for the document advice stream and `sources.tsv`. | `./news` |
| `ORPHEUS_NEWS_AUTO` | `0` disables the automatic 30-minute news refresh (the wire then moves only on `latte news fetch`). | on |
| `ORPHEUS_DATA_AUTO` | `0` disables the automatic 6-hour refresh of market/price series. | on |
| `ORPHEUS_DB_DIR` | directory for the durable database logs. | `./dbdata` |

## Security (see docs/security.md)

| variable | effect | default |
|----------|--------|---------|
| `ORPHEUS_PSK` | pre-shared key for the gossip channel. When set, every peer link is mutually authenticated and encrypted, and it derives the web token too. | unset (plaintext gossip; a non-loopback listener warns) |
| `ORPHEUS_TOKEN` | explicit token for the web console. Required for any non-loopback GUI bind (otherwise the bind is refused). | derived from `ORPHEUS_PSK`, else unset |

## Distribution (see docs/distributed-execution.md)

| variable | effect | default |
|----------|--------|---------|
| `ORPHEUS_WORKERS` | comma-separated `HOST:PORT` workers to register at startup; distribution then runs by default for distributable programs. | none |
| `ORPHEUS_DIST` | `0` forces local execution even when workers are registered. | on when workers exist |
| `ORPHEUS_DIST_NS` / `ORPHEUS_PROFILE_NS` | namespaces isolating a run's worker registry / profiler tables. | default |
| `ORPHEUS_DIST_AUDIT` | `1` logs every distribution decision. | off |
| `ORPHEUS_NO_SPAWN` | `1` prevents a node from spawning helper processes (defense-in-depth for sandboxes). | off |
| `ORPHEUS_REGISTRY` / `ORPHEUS_REGISTRY_KEY` / `ORPHEUS_REGISTRY_MAX` | the shared build/worker registry location, its auth key, and its size cap. | off |

## Set by Orpheus (read by the app / tooling)

| variable | meaning |
|----------|---------|
| `ORPHEUS_URL` | printed by `latte android` (and parsed by the app's WebView) — the loopback URL the GUI is serving on. |

## Other

| variable | effect | default |
|----------|--------|---------|
| `ORPHEUS_PHON` | selects the phonology/sound-change data set for the SCArs engine. | default |
| `ORPHEUS_SCA` | overrides the sound-change rule file. | `lib/ligurian.sca` |
