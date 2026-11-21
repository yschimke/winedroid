# Wear OS

[Back to README](../README.md)

This page covers what winedroid models for Wear OS today, the known gaps, and
a design for running a paired phone and watch.

Single-device watch (implemented)
---------------------------------

A prefix with `"device": "watch"` in `config.json` (see [usage](usage.md)) is a
Wear OS device. The device and the display shape are both immutable session
values, like the display size.

| Android contract | What winedroid does | Source |
| --- | --- | --- |
| `Configuration.uiMode` type | `UI_MODE_TYPE_WATCH` (`0x06`), combined with the configured night bit | `UiModeManagerService.java#updateConfigurationLocked` |
| `Configuration.screenLayout` round bits | `SCREENLAYOUT_ROUND_YES` or `_NO` from the display shape; always one of the two, as on a device | `DisplayContent.java#computeScreenConfiguration` |
| `Configuration.isScreenRound()` | reads the receiver's `screenLayout` | `Configuration.java#isScreenRound` |
| `-watch` resources | `uiMode` type is a real match/precedence input. Before this change it was ignored, so a phone could pick a `-watch` value on a tie | `ResourceTypes.cpp#ResTable_config::match, isBetterThan` |
| `-round` / `-notround` resources | `ResTable_config.screenLayout2` is parsed and matched in both the runtime and the layout projection | `ResourceTypes.h#screenLayout2, MASK_SCREENROUND` |
| `PackageManager.hasSystemFeature` | `android.hardware.type.watch` is true on a watch. Every other feature is still false | `PackageManager.java#FEATURE_WATCH` |
| Round panel | the host masks every presented, dumped and screenshotted frame to the inscribed circle, and pointer presses outside it are dropped | host concern; the runtime still sees a square surface, as on a device |

The default watch display is `227x227` round, the logical size of the 454px
xhdpi "Wear OS Large Round" emulator. winedroid runs at 160 dpi, so logical px
equal dp.

How to check it: a plain-Views probe APK with `values/`, `values-watch/` and
`values-round/` strings, plus runtime calls to `isScreenRound()`,
`hasSystemFeature(FEATURE_WATCH)` and `uiMode & UI_MODE_TYPE_MASK`. Headless,
it renders `form:default shape:default round=false watch=false type=1` on a
phone prefix and `form:watch shape:round round=true watch=true type=6` on a
watch prefix.

Known single-device gaps
------------------------

Each gap is open source, so it can be built by the normal workflow in
[AGENTS.md](../AGENTS.md). Build each one only after a benchmark Wear APK
reaches it.

- **Rotary input.** The host wheel still scrolls the projected surface
  directly. A crown is a `MotionEvent` with `ACTION_SCROLL`,
  `SOURCE_ROTARY_ENCODER` and `AXIS_SCROLL`, routed through
  `View.dispatchGenericMotionEvent` to the focused view. Compose's
  `onRotaryScrollEvent` and `rotaryScrollable` consume it there.
- **Stem and back buttons.** `KEYCODE_STEM_PRIMARY`, `KEYCODE_STEM_1..3`, and
  back as a hardware key rather than a gesture.
- **Framework watch themes.** The probe still reserves an action-bar band on a
  watch. Before changing anything, capture what a Wear emulator draws for
  `Theme.DeviceDefault` (see "UI truth" in AGENTS.md).
- **`uses-library com.google.android.wearable`.** winedroid does not model
  shared libraries. An APK that marks it `required="true"` installs anyway,
  but a device without the library would refuse it. Ambient mode
  (`AmbientLifecycleObserver`) reaches into this closed library.
- **Tiles, watch faces, complications.** `androidx.wear.tiles` / `protolayout`
  and `androidx.wear.watchface` are open source in androidx. All three are
  services that the system binds, and winedroid has no `bindService` yet
  (see below).

Paired phone and watch (design)
-------------------------------

### What "paired" means to an app

Phone and watch apps talk through the Wearable Data Layer:

- `NodeClient`: who is connected
- `MessageClient`: one-shot `path + bytes`, and request/response
- `DataClient`: replicated, versioned `DataItem`s at `wear://<node>/<path>`, with assets
- `CapabilityClient`: which node offers which capability
- `ChannelClient`: byte streams and files

The receiving side gets these as `WearableListenerService` callbacks. The
service is declared in the manifest with
`com.google.android.gms.wearable.MESSAGE_RECEIVED`, `DATA_CHANGED` and similar
actions, plus path filters. It runs even when no activity is on screen.

None of this is in AOSP. It is `play-services-wearable`, compiled into the APK
(R8-minified). It calls GMS Core through `bindService` with the intent action
`com.google.android.gms.wearable.BIND`, which returns an `IWearableService`
binder. Both sides are closed source. That conflicts directly with "Android
behavior is never guessed... it is read", so the design keeps the closed part
as small and as evidence-driven as possible.

### Topology: two processes, two prefixes, one link

```text
 prefix ~/.winedroid/phone   (device: phone)        prefix ~/.winedroid/watch   (device: watch)
 +-----------------------------+                    +-----------------------------+
 | winedroid process: phone    |                    | winedroid process: watch    |
 |  app VM + framework model   |                    |  app VM + framework model   |
 |  wearable service (owned)   |<-- node link ----->|  wearable service (owned)  |
 +-----------------------------+   unix socket      +-----------------------------+
            \___________ pairing.json in each prefix names the peer ___________/
```

- **One process per device, not two runtimes in one process.** The runtime
  keeps the active surface, click handler, loop state and more in
  `thread_local!` state (17 blocks), and assumes one session per process.
  Separate processes also match the real system: two devices, two
  `system_server`s, two package managers and two app data trees. Each prefix
  already carries its own `config.json`, so one is a phone and the other a
  watch with no new config machinery.
- **The link** is a host-owned Unix domain socket, rendezvoused through a
  `pairing.json` that `winedroid pair <phone-prefix> <watch-prefix>` writes
  into both prefixes. It holds the peer's node id (stable, derived from the
  prefix), display name and socket path. Pairing persists across launches as
  it does on devices, and either side can start first. The link is just
  another narrow host bridge, beside today's `host_bridge.rs`.
- **Launch UX:** `winedroid pair --phone app.apk --watch wear.apk` starts both
  processes, giving two windows (the watch one round). The browser build
  would run two wasm instances in one page, connected by a
  `BroadcastChannel`.

### The node bus (winedroid-owned, documented semantics)

The bus implements the semantics in the public Data Layer documentation, not
any GMS internals:

| Concept | Bus model |
| --- | --- |
| Node | `{id, displayName, isNearby}`. `isNearby` follows the link state, so disconnects can be injected for tests |
| Message | `{source, target, path, bytes, requestId?}`, ordered per link. `sendRequest` waits for one reply frame |
| DataItem | `{uri, bytes, assets, version}`, replicated to every node. The higher version wins, with ties broken by node id. Changes and deletes become `DataEvent`s. Persisted in each prefix (`wearable/dataitems.sqlite`), so a restarted side resyncs as a real watch does |
| Capability | static ones are read from the APK's `android_wear_capabilities` string-array (normal resources, through `resource_table`), plus `addLocalCapability` at runtime |
| Channel | a byte stream multiplexed on the link; `sendFile` / `receiveFile` stream from the guest filesystem |

The bus is fully specifiable and testable without any app. A debug CLI such
as `winedroid wearable send --prefix ~/.winedroid/watch /path bytes` can drive
one side.

### Generic runtime prerequisites (open source, citable)

None of these exist yet, and each is valuable beyond Wear:

1. **Service components.** `startService`, `bindService`, `ServiceConnection`,
   `onBind` and `unbindService`, scheduled on the main-thread queue as
   `ActivityThread#handleCreateService` / `handleBindService` work. Tiles and
   watch faces need this too.
2. **Binder and Parcel.** `Parcel` marshalling, `Binder.transact` /
   `onTransact`, and `IInterface` proxies. The GMS client is ordinary app
   bytecode issuing transactions, so this is the seam where winedroid can stand
   in for GMS Core, just as it stands in for `system_server`.
3. **Windowless process lifetime.** A watch app must receive `onMessageReceived`
   with no activity in front. The headless session mode is the starting point.
4. **Package queries for `com.google.android.gms`.**
   `GoogleApiAvailability` checks that the package exists, its version and its
   signature before any binding happens.

### The closed part: a winedroid-owned `IWearableService`

With (1) and (2) in place, binding `com.google.android.gms.wearable.BIND`
returns a winedroid-owned binder that turns transactions into bus operations.
This is the only route that runs unmodified Play Store APKs. The alternatives
are worse:

- Intercepting the public `com.google.android.gms.wearable.*` methods as
  builtins depends on library internals, breaks on minification, and is an
  "APK-specific hack" under AGENTS.md.
- Requiring apps to use a winedroid SDK means they are no longer unmodified
  APKs.

There is no source to cite for the transaction codes and parcel layouts, so
the evidence rule is replaced by **recorded oracles**: transcripts of the
binder traffic between a real app and GMS Core on a paired emulator, stored
under `tests/goldens/<package>/wearable/` with the GMS version they were
captured against. The service should report unsupported transactions as
missing primitives with their code and descriptor, exactly like a missing
framework method. It must never answer them with a fake success.

### Testing the pair

- **Journeys** gain a device prefix for steps (`@watch tap text "Start"`,
  `@phone waittext "Synced"`). One file drives both processes, because
  cross-device behavior is the thing under test.
- **Real-Android oracle:** a phone emulator and a Wear emulator paired through
  the Wear OS companion app and `adb forward tcp:5601 tcp:5601`.
  `capture_oracle.sh` extends to two serials and compares both devices'
  checkpoints.
- **Bus contract tests** cover ordering, `DataItem` conflict resolution,
  resync after reconnect and capability discovery. Each test states the
  documented behavior it asserts.

### Suggested order

1. Done: single-device watch (this branch).
2. Rotary and stem input. These are small, open, and reached by any Compose
   for Wear app.
3. Service and Binder model (prerequisites 1 and 2), proven first on
   something open such as a Tiles host.
4. `winedroid pair`: two processes, link, node bus and debug CLI, with no GMS.
5. GMS `IWearableService` emulation driven by recorded transcripts, starting
   with `NodeClient` and `MessageClient`, the smallest useful slice.
