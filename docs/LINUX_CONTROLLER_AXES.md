# Linux controller axes

The Linux in-game options offer an **Axis mode** selector for each configured device under **Controls / Game controllers**. Select the device tab after setting it up. Mode and **Invert** changes are saved immediately and retained after restarting. Invert is available even before assigning a function.

The bundled Electron launcher uses the existing controller page; its protobuf protocol accepts the optional `axis_mode` field. Until that separate UI offers a selector, use the in-game options to change the mode. Saving from an older launcher preserves the selected mode.

| Mode | Axis input |
| --- | --- |
| Auto | Use native evdev axes when there is specific wheel, pedal or joystick evidence, or when gilrs has no mapping. Otherwise retain the existing gilrs processing. |
| Gamepad | Force gilrs processing and its stick/trigger layout. |
| Native | Read the advertised native axes directly, regardless of device classification or force feedback support. |

The per-device `controller` settings contain `axis_mode = "auto"`, `"gamepad"` or `"native"`. Missing, invalid and unknown values select Auto. Existing assignments, calibration, inversion, curve flags and force feedback settings remain in the existing format. Changing mode does not erase them; assignments made for a different layout may need adjustment. Importing OMSI assignments retains the existing native mode, calibration and neoOMSI feedback settings, including inversion flags on unassigned imported axes.

## Auto selection and classification

Force feedback is never evidence for choosing an axis mode. Auto uses the actual OS device name and advertised axes/buttons. Explicit wheel/gearing controls, gas/brake axes, and joystick controls without gamepad buttons support native input. A mapped controller with normal gamepad buttons and two sticks remains on gilrs when gas/brake axes alone would be ambiguous.

Recognized racing/steering wheel names, Driving Force, Thrustmaster T128/T248, and pedal names also require matching driving axes. A name alone is insufficient. Unknown mapped devices stay on gilrs. Specific wheel names with matching driving axes take precedence over gamepad button and extra-axis reports. Vendor-wide classification is not used.

Native axis layout and gamepad classification are separate. In Native mode, gamepad buttons and X/Y stick axes retain gamepad steering unless specific wheel or pedal evidence is present. An unknown mapped wheel without this gamepad evidence uses wheel steering when Native is selected. Auto keeps the conservative gilrs fallback for unknown mapped devices; Gamepad explicitly forces it. Native axes retain their X/Y/Z/etc. labels. Assign functions to those slots; mapped stick/trigger defaults are excluded so they cannot overwrite or mix with native assignments. Force feedback does not affect this decision. A native read failure leaves its axes unavailable instead of silently switching layouts or reusing stale values.

## Cache and snapshots

Each connection gets one cached evdev reader, capabilities, axis inventory and reported ranges. Successful and permanently unsupported results are retained without periodic capability checks. Both gilrs Connected and Disconnected events invalidate the entry, including when an ID and path are reused within one event drain. A changed path also replaces the entry. Saved mode changes are synchronized on regular polls and when the game reloads controller settings, without reopening the device or querying capabilities again.

Temporary errors such as permission failures, an opening race or interrupted I/O permit at most three attempts, spaced by 500 ms. Other errors stop retries until reconnect. Initial probing, force feedback probing and current-value reads have separate retry state. Failures while reading values retain the capability cache and clear the displayed snapshot. A successful current-value read resets its consecutive failure budget.

Native input uses `EVIOCGABS` for every inventoried axis immediately upon opening and on regular polls. This provides current values even when no movement event has occurred. The returned minimum and maximum are cached with the sample and updated when the driver changes them, such as after adjusting steering rotation. Values normalize to -1..1 using 64-bit arithmetic for the range, with clamping and zero for invalid ranges.

Only advertised ABS codes X through BRAKE are considered. The native-to-OMSI mapping preserves X/Y/Z/Rx/Ry/Rz and slider positions. Hats and miscellaneous reports are excluded. Checking the bitmap first matters: `EVIOCGABS` can succeed with zeroed data for absent codes.

Force feedback capability is queried separately and cached, regardless of axis mode. The existing evdev steering backend requires an advertised `FF_CONSTANT` effect; gilrs rumble support is retained independently. Native input never implies that force feedback is available. The axis reader opens devices read-only and sends no motor commands.

## ioctl requests

`eviocgabs(axis)` corresponds to Linux UAPI `EVIOCGABS(axis)`: `_IOR('E', 0x40 + axis, struct input_absinfo)`. The buffer contains the current value, minimum, maximum, fuzz, flat and resolution as six signed 32-bit integers.

`eviocgbit<N>(event_type)` corresponds to `EVIOCGBIT(event_type, N)`: `_IOC(_IOC_READ, 'E', 0x20 + event_type, N)`. For a fixed `[u8; N]` buffer this is equivalent to `_IOR('E', 0x20 + event_type, [u8; N])`.

Both helpers use the existing `libc::_IOR` implementation. It encodes the read direction, input subsystem type, request number and buffer size using the target architecture's ioctl layout, avoiding hand-written bit shifts and new dependencies.

Sources: [Linux input UAPI](https://github.com/torvalds/linux/blob/master/include/uapi/linux/input.h), [ioctl encoding](https://github.com/torvalds/linux/blob/master/include/uapi/asm-generic/ioctl.h), [input event protocol](https://www.kernel.org/doc/html/latest/input/event-codes.html), [gamepad specification](https://www.kernel.org/doc/html/latest/input/gamepad.html).

Native axis reading, mode selection and independent inversion for unassigned axes are compiled only on Linux. The optional Protobuf field is ignored on other platforms. Windows DirectInput, settings behavior and previews retain their existing behavior.
