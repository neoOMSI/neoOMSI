# Launcher protocol

The external launcher (Electron, [neoOMSI/launcher](https://github.com/neoOMSI/launcher)) starts the
engine as

```text
neoomsi --control-protocol
```

and talks to it over the child's stdin and stdout. Games the engine starts report back to it over
a loopback link. Version: **1**.

```text
launcher ──stdin/stdout──▶ neoomsi --control-protocol ──127.0.0.1──▶ neoomsi (game)  ×n
```

## Framing

Every message is a 4-byte big-endian length followed by that many bytes of UTF-8 JSON. A frame
may be at most 16 MiB. A broken frame ends the connection: neither side tries to resynchronise.

```json
{ "type": "maps", "requestId": "req_17", "payload": {}, "error": "…" }
```

| Field       | Meaning                                                             |
| ----------- | ------------------------------------------------------------------- |
| `type`      | the command, or the event's name                                    |
| `requestId` | set on a request and copied onto its answer; never set on an event   |
| `payload`   | the request's arguments, the answer's result, or the event's data   |
| `error`     | on an answer instead of a result: what went wrong, for the player   |

stdout carries frames only. The engine points its own standard output at stderr in this mode, so
anything printed goes to stderr with the log. The launcher shows stderr as diagnostics.

## Session

1. The launcher sends `handshake` first:
   `{"protocolVersion": "1", "launcherVersion": "0.3.0", "clientPlatform": "win32"}`.
   Any other request before it, except `shutdown`, is answered with an error.
2. The engine answers with:

   ```json
   {
     "status": { "code": 0, "message": "OK" },
     "protocolVersion": "1",
     "engineVersion": "0.2.0",
     "supportedCapabilities": ["events.instances", "events.installs", "events.content", "events.session", "game.link"],
     "commands": ["config", "maps", "…"]
   }
   ```

   A different major version is answered with `status.code` 2, and the engine stays unready.
   `commands` lists everything the engine can answer. The launcher does not send others (for
   example `uninstall_mod`, which only its mock has so far).
3. Requests run side by side. Their answers come back in any order, matched by `requestId`.
4. `shutdown` (or closing stdin) ends the engine once the requests still running are answered
   (10 s at most). Games it started keep running, and the next engine finds them through
   `~/.neoomsi/instances`.

## Commands

The shapes are those of `src/types/launcher.ts` in the launcher. Field names are written as Rust
serialises them.

| Command | Arguments | Notes |
| --- | --- | --- |
| `config`, `save_config` | `{root?, game?, profile?}` | saving also drops the cached content lists |
| `maps`, `vehicles`, `weather` | – | cached until the content changes (`content_changed`) |
| `lines` | `{map, date}` | |
| `minimap` | `{map, date?}` | roads (`main`: a speed limit of 55 km/h or more), stops and entry points in world metres, each with the `--spawn` it starts at; kept per map and date until the content changes |
| `ibis` | `{bus, hof, line}` | |
| `profiles`, `profile`, `create_profile`, `delete_profile` | `{name, sex?}` | |
| `mods`, `modinfo` | `{path}` | |
| `start_install` (`install`) | `{path, mode?}` | returns at once; progress comes as `installs_changed` |
| `cancel_install`, `clear_installs` | `{id}` | |
| `instances`, `launch`, `stop`, `log` | `Duty` / `{pid}` / `{pid, lines?}` | `stop` asks over the game link first, then by signal |
| `join` | `{text}` | |
| `settings`, `save_settings`, `option_presets` | changed keys | saving `pax_models: "realistic"` downloads the pack when it is missing |
| `pax_pack`, `install_pax_pack` | – | the realistic passengers' pack: `{state, done, total, message, installed, latest}`; `latest` (`{version, notes, page, published}`) is the newest `realistic-pax-v<n>` release, looked for every 6 hours, and makes an older pack `outdated` |
| `update_check` | – | the newest neoOMSI release for this build's channel and platform (`{version, page, notes, prerelease, size}`), or `null` |
| `keybindings`, `save_keybindings`, `controllers`, `save_controllers` | the whole list | `controllers` reads the devices as they are now (the first call waits half a second for them to be found) |
| `preview` | `{bus, paint}` | the path of a `.glb` file |
| `situations` | `{map}` | |
| `tutorials`, `servers`, `save_servers`, `version` | | `servers` asks every server for its status |

## Events

| `type` | `payload` | When |
| --- | --- | --- |
| `instances_changed` | `Instance[]` | the list of games changed (checked every second, and at once after a request or a game's report) |
| `installs_changed` | `InstallProgress[]` | an install moved on (every 250 ms while one runs) |
| `content_changed` | `{stamp}` | maps, buses or weather were added or removed: lists the launcher holds are stale |
| `session_event` | `{sessionId, pid, state, message, progress?, exitCode?}` | a game moved to another state |
| `pax_pack_changed` | as `pax_pack` answers | the realistic passengers' download or install moved on, or a newer release was found |

`state` counts `1` starting, `2` loading, `3` running, `4` stopping, `5` exited, `6` failed.
`sessionId` is the instance id. A game ends as *failed* when it reported a failure, or when it
exited with a non-zero code without being stopped. Each `Instance` also carries `link`: what the
game reported last (`{state, progress, message, window}`), while it is connected.

## Game link

The engine listens on `127.0.0.1` at a free port. Every game it starts gets three environment
variables:

| Variable             | Value                        |
| -------------------- | ---------------------------- |
| `OMSI_INSTANCE`      | the instance id              |
| `OMSI_CONTROL`       | `127.0.0.1:<port>`           |
| `OMSI_CONTROL_TOKEN` | a random token per engine    |

The game connects with the same framing and sends
`hello {instance, token, pid, version}`. The engine answers `welcome`, or `refused` and closes
the connection. From then on:

- game → engine: `state {state, progress, message, window}` with `state` one of `loading` (sent
  at most every 250 ms), `running`, `stopping` or `failed`. `window` turns true once the game's
  window is on screen; the game brings it to the front itself at that moment;
- engine → game: `quit`. The game ends its session the way closing its window does (summary,
  personnel file, LAN goodbye).

The launcher stays on screen after `launch` and steps aside (minimises or hides, as the player
set it) only when that game's `link.window` turns true; a game that never gets a window leaves
the launcher where it is, showing why.

A game without the link (an older build, or one started by hand) still shows up through its
instance file. Its Stop falls back to SIGTERM, or to closing its window on Windows.
