# Dedicated server

neoOMSI includes headless dedicated server capabilities for hosting multiplayer sessions without requiring a graphical display or audio hardware.

## Running the server

Start `neoomsi` with the `--server` flag and specify the path to your server configuration file:

```sh
neoomsi --root /path/to/OMSI2 --server /opt/neoomsi-server/server.cfg
```

On first startup, if `server.cfg` does not exist, a default configuration file is generated with commented parameters.

## Configuration (`server.cfg`)

Common configuration properties:

| Key           | Description                                                      | Default                    |
| ------------- | ---------------------------------------------------------------- | -------------------------- |
| `name`        | Public server name displayed in browser                          | `neoOMSI Server`           |
| `motd`        | Message of the day shown upon connection                         | `Welcome to neoOMSI`       |
| `map`         | Relative path to map (`maps/.../global.cfg`)                     | `maps/Grundorf/global.cfg` |
| `port`        | UDP port for game network traffic                                | `18730`                    |
| `web_port`    | HTTP/WebSocket port for status & browser                         | `18731`                    |
| `max_players` | Maximum concurrent player connections                            | `16`                       |
| `traffic`     | Density of AI traffic (0 to 100)                                 | `50`                       |
| `timetable`   | Enable scheduled AI timetable runs (0 or 1)                      | `1`                        |
| `tunnel`      | Automatically expose server via Cloudflare quick tunnel (0 or 1) | `0`                        |

## Building the server package

Build the dedicated server bundle using the provided build script:

```sh
scripts/build-server.sh
```

This compiles `neoomsi` with headless flags, generates `start.sh`, and outputs the complete server bundle to `dist/server/`.
