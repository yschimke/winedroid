# Usage

[Back to README](../README.md)

The commands below assume `winedroid` is on your `PATH`; when running from a
source checkout, use `./target/release/winedroid` instead.

Commands
--------

Inspect an APK:

```sh
winedroid inspect /path/to/app.apk
```

Run an APK (installs into the default prefix, then launches):

```sh
winedroid /path/to/app.apk
```

Install an APK without launching it:

```sh
winedroid install /path/to/app.apk
```

List installed packages:

```sh
winedroid list
```

Launch an already-installed package from the prefix store:

```sh
winedroid launch com.example.app
```

Display configuration
---------------------

The prefix config at `~/.winedroid/default/config.json` owns the logical Android
display size. Both dimensions are required when `display` is present:

```json
{
  "ui_mode": "system",
  "frame_rate": "host",
  "display": {
    "width": 420,
    "height": 760
  }
}
```

The default is `420x760`. Width and height must each be from 1 through 4096,
and their product must not exceed 2,097,152 logical pixels. The configured size
drives Android display metrics, configuration and resource selection, layout,
input coordinates, rendering, and the host surface as one immutable session
value.

Wear OS devices
---------------

Set `device` to `watch` to run a prefix as a Wear OS device:

```json
{
  "device": "watch",
  "display": {
    "width": 227,
    "height": 227,
    "shape": "round"
  }
}
```

A watch reports `Configuration.UI_MODE_TYPE_WATCH` in `uiMode`, so `-watch`
resources are selected, and `PackageManager.hasSystemFeature` reports
`android.hardware.type.watch`. `display.shape` is `round` or `rectangular`. It
defaults to `round` for a watch and `rectangular` for a phone. A round display
sets `Configuration.SCREENLAYOUT_ROUND_YES`, which selects `-round` resources
and makes `Configuration.isScreenRound()` true. The host draws the panel as a
circle and ignores pointer input outside it. On a watch, the mouse wheel
(or trackpad scroll) is the crown: each wheel notch becomes one rotary
`ACTION_SCROLL` event with `AXIS_SCROLL` of ±1, sent to the focused view, as
on a device. Without a focused view, nothing receives it. When `display` is omitted, a watch
uses a `227x227` round display, the logical size of a 454px xhdpi Wear OS
emulator. See [wear.md](wear.md) for coverage and the paired phone/watch
design.

Uninstall a package:

```sh
winedroid uninstall com.example.app
```

Browser build
-------------

The browser target uses Emscripten so the existing Skia renderer can run on
WebAssembly. Install Emscripten with `emcc` on `PATH`, then install the Rust
target and build the web bundle:

```sh
rustup target add wasm32-unknown-emscripten
scripts/build_web.sh
```

The build script downloads rust-skia's matching Emscripten archive on its first
run and writes `winedroid.js` and `winedroid_web.wasm` under `web/dist`. Serve
the directory over HTTP and open the printed local URL:

```sh
python3 -m http.server 4173 --directory web
```

Choose or drop an APK in the page. The browser build runs DEX through the
interpreter, presents Skia frames on the canvas, and routes pointer, wheel, and
keyboard input through the same activity session used by native hosts. App data
currently lives in Emscripten's in-memory filesystem and is lost on reload.
Cranelift JIT and Android native `.so` execution are not available in the
browser; reaching native code returns an explicit missing-primitive error.
The browser phone defaults to a `420x910` logical display. Add a display query,
for example `?display=360x800`, to choose another valid size without adding page
controls.

Run the generated-module smoke, or include an APK for the end-to-end APK path:

```sh
node scripts/test_web_runtime.mjs
node scripts/test_web_runtime.mjs /path/to/app.apk
```

Installed packages live under the prefix (`~/.winedroid/default/apps/<package>`).
Apps also share an emulated external storage volume at
`~/.winedroid/default/storage` (the guest `/storage/emulated/0`), holding the
standard public directories (`Download`, `Documents`, `DCIM`, ...) plus per-app
`Android/data/<package>` directories. An app only ever sees this volume, never
the host's real home directory.

While an app window is focused, pressing the screenshot shortcut (Command+S on
macOS) saves the current frame as a PNG into the emulated
`Pictures/Screenshots` directory, mirroring Android, and plays a brief on-top
confirmation flash. The capture is the clean app frame and the flash never
touches the running app. (Currently implemented on the macOS window backend.)

Profile an APK launch with runtime timing traces:

```sh
scripts/profile_apk.sh /path/to/app.apk
```

Check runtime prerequisites:

```sh
winedroid runtime-check
```

Probe host ART availability:

```sh
winedroid art-probe
```
