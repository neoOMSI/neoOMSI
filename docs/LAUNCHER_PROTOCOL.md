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

## Schema

[`crates/launcher-protocol/proto/launcher.proto`](../crates/launcher-protocol/proto/launcher.proto)
defines every message: the frame, each command's arguments and answer, the handshake and the
events. It is the contract. The engine's Rust types are generated from it when the crate builds
(prost, compiled by protox, so no `protoc` is needed), and the launcher keeps a copy of the file
(`pnpm sync:engine`) from which it generates its TypeScript; the launcher's CI fails when that
TypeScript is out of date. A protocol change is a change to the `.proto`, synced into the
launcher. The game link's messages are in
[`game_link.proto`](../crates/launcher-protocol/proto/game_link.proto) next to it: only the engine
and the games it starts speak them, so the launcher does not copy that file.

## Framing

Every message is a 4-byte big-endian length followed by that many bytes of a protobuf `Frame`. A
frame may be at most 16 MiB. A broken frame ends the connection: neither side tries to
resynchronise.

| `Frame` field | Meaning |
| --- | --- |
| `request_id` | set on a request and copied onto its answer; empty on an event |
| `error` | on an answer instead of a `response`: what went wrong, for the player |
| `request` | a `Request`: one command and its arguments |
| `response` | a `Response`: the answer, under the same name as the command |
| `event` | an `Event` |

stdout carries frames only. The engine points its own standard output at stderr in this mode, so
anything printed goes to stderr with the log. The launcher shows stderr as diagnostics.

## Session

1. The launcher sends `handshake` first, with `protocol_version` `"1"`, its own version and its
   platform. Any other request before it, except `shutdown`, is answered with an error.
2. The engine answers with `status`, its `protocol_version`, `engine_version`,
   `supported_capabilities` (`events.instances`, `events.installs`, `events.content`,
   `events.session`, and `game.link` when the game link is up) and `commands`, the names of every
   command it answers. A different major version is answered with
   `STATUS_CODE_UNSUPPORTED_VERSION`, and the engine stays unready. The launcher does not send
   commands the engine does not list (`uninstall_mod`, for example, only its mock has so far).
3. Requests run side by side. Their answers come back in any order, matched by `request_id`.
4. `shutdown` (or closing stdin) ends the engine once the requests still running are answered
   (10 s at most). Games it started keep running, and the next engine finds them through
   `~/.neoomsi/instances`.

A launcher of the earlier JSON protocol sends JSON, which is no `Frame`: the engine ends the connection, and the
launcher shows that the engine went away.

## Commands

| Command | Notes |
| --- | --- |
| `config`, `save_config` | saving the folders also drops the cached content lists |
| `maps`, `vehicles`, `weather` | cached until the content changes (`content_changed`) |
| `lines`, `ibis` | |
| `minimap` | roads (`main`: a speed limit of 55 km/h or more), stops and entry points in world metres, each with the `--spawn` it starts at; kept per map and date until the content changes |
| `profiles`, `profile`, `create_profile`, `delete_profile` | |
| `mods`, `modinfo` | |
| `start_install` | returns at once; progress comes as `installs_changed` |
| `cancel_install`, `clear_installs` | |
| `instances`, `launch`, `stop`, `log` | `stop` asks over the game link first, then by signal |
| `join` | what a join field means, and the LAN sessions hosted here |
| `settings`, `save_settings`, `option_presets` | `Settings` has a field for every setting the engine keeps; saving changes only the fields that are set, and saving `pax_models` realistic downloads the pack when it is missing |
| `pax_pack`, `install_pax_pack` | the realistic passengers' pack; `latest` is the newest `realistic-pax-v<n>` release, looked for every 6 hours, and makes an older pack `outdated` |
| `update_check` | the newest neoOMSI release for this build's channel and platform, if there is one, and `update`: the state of an installation, `UPDATE_STATE_FAILED` with the reason when the last one did not go in |
| `install_update` | downloads that release (progress as `update_changed`), then starts `neoomsi --finish-update`, which puts it in place once the process `launcher_pid` has ended and starts neoOMSI again; the launcher quits on `UPDATE_STATE_RESTARTING`, since its own folder is among the files replaced |
| `keybindings`, `save_keybindings`, `controllers`, `save_controllers` | the whole list; `controllers` reads the devices as they are now (the first call waits half a second for them to be found), each axis with its raw value and calibration; an Xbox-type pad is read through XInput, which works without a window |
| `preview` | the path of a `.glb` file |
| `situations`, `tutorials`, `version` | |
| `servers`, `save_servers` | `servers` asks every server for its status |

## Events

| `Event` | When |
| --- | --- |
| `instances_changed` | the list of games changed (checked every second, and at once after a request or a game's report) |
| `installs_changed` | an install moved on (every 250 ms while one runs) |
| `content_changed` | maps, buses or weather were added or removed: lists the launcher holds are stale |
| `session_event` | a game moved to another state |
| `pax_pack_changed` | the realistic passengers' download or install moved on, or a newer release was found |
| `update_changed` | the neoOMSI download moved on, failed, or is ready for the restart |

A game ends as `SESSION_STATE_FAILED` when it reported a failure, or when it exited with a
non-zero code without being stopped. Each `Instance` also carries `link`, what the game reported
last while it is connected, and `lan_status`, the LAN status file the game writes while a session
runs.

## Game link

The engine listens on `127.0.0.1` at a free port. Every game it starts gets three environment
variables:

| Variable             | Value                        |
| -------------------- | ---------------------------- |
| `OMSI_INSTANCE`      | the instance id              |
| `OMSI_CONTROL`       | `127.0.0.1:<port>`           |
| `OMSI_CONTROL_TOKEN` | a random token per engine    |

The game connects with the same 4-byte length framing, around a `FromGame` from the game and a
`ToGame` from the engine. It sends `hello` (`GameHello`). The engine answers `welcome`, or
`refused` with the reason and closes the connection. A hello must arrive within 5 s and be at
most 4 KiB. From then on:

- game → engine: `state`, the same `GameLink` the launcher gets in `Instance.link`, with `state`
  loading (sent at most every 250 ms), running, stopping or failed. `window` turns true once the
  game's window is on screen; the game brings it to the front itself at that moment;
- engine → game: `quit`. The game ends its session the way closing its window does (summary,
  personnel file, LAN goodbye).

The launcher stays on screen after `launch` and steps aside (minimises or hides, as the player
set it) only when that game's `link.window` turns true; a game that never gets a window leaves
the launcher where it is, showing why.

A game without the link (an older build, or one started by hand) still shows up through its
instance file. Its Stop falls back to SIGTERM, or to closing its window on Windows.
