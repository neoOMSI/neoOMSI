# User guide

This guide covers running neoOMSI, essential keybindings, and common configuration options.

> [!IMPORTANT]
> **neoOMSI requires an existing OMSI 2 installation.** neoOMSI does not distribute copyrighted game content. On first launch, you must provide the path to your OMSI 2 installation folder.

## Getting started

1. Download the build for your operating system from the [Download page](https://neoomsi.com/download/) or GitHub [Releases](https://github.com/neoOMSI/neoOMSI/releases).
2. Extract the archive into a dedicated folder with write permissions (e.g. inside your user directory, not `Program Files`).
3. Launch `neoomsi` (`neoomsi.exe` on Windows).
4. On first start, select your OMSI 2 installation folder (the directory containing `maps` and `Vehicles`). neoOMSI reads these files without modifying them.
5. Select a map, vehicle, and timetable duty, then start driving.

> [!NOTE]
> **Linux:** the launcher runs in Chromium's sandbox. If the system restricts user namespaces (Ubuntu 24.04 and later), configure the sandbox helper once from the neoOMSI folder:
>
> ```sh
> sudo chown root:root launcher/chrome-sandbox && sudo chmod 4755 launcher/chrome-sandbox
> ```

## Keybindings

### Driving controls

| Action             | Primary Key | Alternative                   |
| ------------------ | ----------- | ----------------------------- |
| **Throttle**       | `W`         | `Up Arrow`                    |
| **Brake**          | `S`         | `Down Arrow`                  |
| **Steer Left**     | `A`         | `Left Arrow`                  |
| **Steer Right**    | `D`         | `Right Arrow`                 |
| **Mouse Steering** | `O`         | Toggles mouse steering on/off |

### Vehicle operations

| Action                 | Key         | Description                              |
| ---------------------- | ----------- | ---------------------------------------- |
| **Battery / Ignition** | `E`         | Inserts key and powers electrical system |
| **Engine Starter**     | `M`         | Hold to crank engine until started       |
| **Drive Gear (D)**     | `Shift + D` | Engages forward drive                    |
| **Neutral (N)**        | `N`         | Neutral gear                             |
| **Reverse (R)**        | `R`         | Reverse gear                             |
| **Parking Brake**      | `.`         | Toggles handbrake                        |
| **Quick Autostart**    | `Shift + U` | Automates the complete startup sequence  |

### Camera & cockpit

- **Cockpit switches:** Left-click to toggle, click and drag to turn rotary dials.
- **Look around:** Hold Right-Mouse-Button and move mouse (or arrow keys / `I`/`J`/`K`/`L`).
- **In-game menu:** Press `Esc` to access settings, switch buses, or exit.

## Trip evaluation

At the final stop of a scheduled trip, stop the bus and open a passenger door to
show the trip evaluation. Service trips and vehicles without a passenger cabin
only require stopping. The screen pauses single-player simulation; LAN sessions
continue running.

The table lists every planned stop with planned and actual arrival/departure
times (`HH:MM:SS`), signed differences in seconds, and a status. Positive
differences mean late; negative differences mean early. Day offsets identify
trips crossing midnight. Missing observations appear as `-`; the final departure
has no actual time when the report opens on arrival.

Use the in-game menu to view the current trip or the last completed trip. Scroll
with the mouse wheel, arrow keys, or Page Up/Page Down. Select **Save as text…**
(or press `Ctrl+S`) to export the table. **Continue** or `Esc` resumes driving.

Observations are recorded while a duty is active. Earlier observations are not
reconstructed when loading a saved situation or joining a trip partway through.
The last completed report remains available during the current session; save it
to a text file to keep it after exiting.

## Passenger seating

Under **Settings → Gameplay → Passengers**, enable **Passengers prefer available seats**
to reserve free seats before using standing places when passengers board. Standing places
are used once all seats are occupied or reserved. This neoOMSI option is off by default;
with it off, passengers choose randomly among all free places, as in OMSI 2.

The option can also be changed through the in-game **Options → Gameplay** menu. Changes
apply to subsequent place reservations in player and timetable buses. In `settings.cfg`,
the option is stored as `pax_prefer_seats=1` (enabled) or `pax_prefer_seats=0` (disabled).

**Boarding at the rear doors** (on by default) lets passengers who buy no ticket from the
driver get on at the exit doors too. Turned off, everybody boards at the bus's entries, as
in OMSI 2. It is stored as `pax_rear_entry`.

## Wet-road reflections

Vanilla, Vanilla+ and Enhanced show reflections of buses, buildings and scenery in
wet-road puddles. Enable reflections in the graphics settings, or set `reflections=1`
in `settings.cfg`.

Puddle reflections remain visible from inside the bus through its windows and rain
films. Glass tint and raindrops affect the view, including the reflections outside.
Each graphics mode keeps its own lighting and colour treatment.

## Command-line options

You can launch directly into a specific scenario using command-line arguments:

```sh
neoomsi --map maps/Grundorf/global.cfg --bus Vehicles/MAN_SD200/MAN_SD80.bus
```

| Flag                  | Description                                                  |
| --------------------- | ------------------------------------------------------------ |
| `--root <path>`       | Path to the OMSI 2 base directory                            |
| `--map <path>`        | Path to the map global configuration (`maps/.../global.cfg`) |
| `--bus <path>`        | Vehicle file to load (`Vehicles/.../*.bus`)                  |
| `--weather <path>`    | Weather profile to apply (`Weather/*.owt`)                   |
| `--time <HH:MM>`      | Initial simulation time                                      |
| `--date <YYYY-MM-DD>` | Initial simulation date                                      |
| `--enhanced`          | Enable enhanced physically based rendering mode              |

## Modding

Install mods on the launcher's **Mods** page (an archive - `.zip`, `.7z`, `.rar` - or a folder). Each mod
goes into the `mods` folder of neoOMSI's content folder, one folder or `.zip` per mod, exactly as it
was packed; neoOMSI finds the folder with OMSI's content (`Vehicles`, `maps`, `Sceneryobjects` ...)
inside it by itself. The content folder is the folder of the `neoomsi` program (its `neoOMSI`
folder when the program sits in the OMSI 2 folder), `~/.neoomsi/content` when that folder cannot be
written, or the folder `OMSI_CONTENT` names. A folder or `.zip` copied into `mods` by hand works
the same way. The original OMSI 2 files are never changed.

**Load order.** The Mods page lists your mods by priority: an entry higher in the list wins every
file it shares with the entries below it, and with the original OMSI 2 files - for example, put an
add-on above the mod it changes. Files no higher entry has come from the ones below, so a mod can use
the files of another mod. You decide the order (drag an entry or use its arrows, then **Apply**); the
launcher never reorders it. A newly installed mod goes to the top or the bottom, as you choose
there, and an updated mod keeps its place. The order is kept in `mods/.load-order.json`; **Reset**
returns to the default (archives of the old `Archives` folder, then the mods alphabetically). Each entry shows which files it replaces and which are hidden by entries above it.

A mod's plugins (program code that runs in the game) load only after you allow them for that mod
on the Mods page. Uninstalling moves a mod to `mods/uninstalled`; nothing is deleted. Changes to
the installed mods and the load order take effect the next time a game starts.

When content a map or bus needs cannot be found, the game says so once after loading, and the
session's entry on the launcher's Sessions page shows what is missing: a pack that is not
installed, one that lies in `mods/uninstalled`, or files missing in a mod. The Drive page checks
the chosen map before the start (the first check of a big map takes a moment). A finding you
dismiss is not shown again, unless what is wrong changes.

Set `OMSI_NO_SURF=1` before starting neoOMSI to disable OMSI `.surf` height maps for an A/B comparison of wheel contact.

## Passenger models and movement

**Settings → Gameplay → Passenger movement** selects Natural or OMSI 2 movement.
**Procedural passenger animation** independently selects the pose system; `pax_ik`
and `--pax-ik` control it (`ik` remains a settings alias).

For realistic models, press **Download the realistic passengers** under
**Settings → Gameplay** in the launcher (a few hundred MB, once). The launcher installs
them into the content folder's `Packs/RealisticPax` and selects **Passenger models →
Realistic**; they appear from the next game start. Games that are running then show
the old passengers until they are restarted: the launcher offers **Restart the game now**,
which ends their drives and starts each of them again as it was started; games started
after the change are left running. Licensing details are in
[RealisticPax](../tools/realistic-pax/README.md). Without the pack, installed OMSI
passengers remain in use.
