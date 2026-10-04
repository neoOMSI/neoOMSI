# User guide

This guide covers running neoOMSI, essential keybindings, and common configuration options.

> [!IMPORTANT]
> **neoOMSI requires an existing OMSI 2 installation.** neoOMSI does not distribute copyrighted game content. On first launch, you must provide the path to your OMSI 2 installation folder.

## Getting started

1. Download the latest release from the [Releases](https://github.com/neoOMSI/neoOMSI/releases) page for your operating system.
2. Extract the archive into a folder with write permissions (e.g. within your user directory).
3. Launch `neoomsi` (`neoomsi.exe` on Windows).
4. If prompted, select your OMSI 2 installation directory (containing `Omsi.exe` and `maps/`).
5. Select a map, vehicle, and duty, then start the simulation.

## Keybindings

### Driving controls

| Action | Primary Key | Alternative |
| --- | --- | --- |
| **Throttle** | `W` | `Up Arrow` |
| **Brake** | `S` | `Down Arrow` |
| **Steer Left** | `A` | `Left Arrow` |
| **Steer Right** | `D` | `Right Arrow` |
| **Mouse Steering** | `O` | Toggles mouse steering on/off |

### Vehicle operations

| Action | Key | Description |
| --- | --- | --- |
| **Battery / Ignition** | `E` | Inserts key and powers electrical system |
| **Engine Starter** | `M` | Hold to crank engine until started |
| **Drive Gear (D)** | `Shift + D` | Engages forward drive |
| **Neutral (N)** | `N` | Neutral gear |
| **Reverse (R)** | `R` | Reverse gear |
| **Parking Brake** | `.` | Toggles handbrake |
| **Quick Autostart** | `Shift + U` | Automates the complete startup sequence |

### Camera & cockpit

- **Cockpit switches:** Left-click to toggle, click and drag to turn rotary dials.
- **Look around:** Hold Right-Mouse-Button and move mouse (or arrow keys / `I`/`J`/`K`/`L`).
- **In-game menu:** Press `Esc` to access settings, switch buses, or exit.

## Passenger seating

Under **Settings → Gameplay → Passengers**, enable **Passengers prefer available seats**
to reserve free seats before using standing places when passengers board. Standing places
are used once all seats are occupied or reserved. This neoOMSI option is off by default;
with it off, passengers choose randomly among all free places, as in OMSI 2.

The option can also be changed through the in-game **Options → Gameplay** menu. Changes
apply to subsequent place reservations in player and timetable buses. In `settings.cfg`,
the option is stored as `pax_prefer_seats=1` (enabled) or `pax_prefer_seats=0` (disabled).

## Command-line options

You can launch directly into a specific scenario using command-line arguments:

```sh
neoomsi --map maps/Grundorf/global.cfg --bus Vehicles/MAN_SD200/MAN_SD80.bus
```

| Flag | Description |
| --- | --- |
| `--root <path>` | Path to the OMSI 2 base directory |
| `--map <path>` | Path to the map global configuration (`maps/.../global.cfg`) |
| `--bus <path>` | Vehicle file to load (`Vehicles/.../*.bus`) |
| `--weather <path>` | Weather profile to apply (`Weather/*.owt`) |
| `--time <HH:MM>` | Initial simulation time |
| `--date <YYYY-MM-DD>` | Initial simulation date |
| `--enhanced` | Enable enhanced physically based rendering mode |

## Modding

Place add-on content into the `Mods/` directory alongside the `neoomsi` executable. neoOMSI mounts add-ons into its virtual filesystem without altering original OMSI 2 files.

Set `OMSI_NO_SURF=1` before starting neoOMSI to disable OMSI `.surf` height maps for an A/B comparison of wheel contact.
