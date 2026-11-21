winedroid — agent instructions
==============================

This file is the operating manual for anyone, human or AI, writing code in
this repo. Read it before touching code.

- Architecture (runtime boundary, layer ownership, runtime contract,
  refactor pressure) lives in docs/architecture.md. Read it before structural work.
- todo.txt is a task list only. All instructions live here.

Coding rules
------------

- Implement real Android/runtime behavior from source. Best-effort execution
  is the development default; WINEDROID_STRICT=1 makes missing support fatal.
  Conformance tests and validation must use strict mode.
- Do not add APK-specific hacks: no package-name, app-name, resource-name, or
  screen-specific branches; no fake colors, fake themes, fake stubs, or
  "close enough" equivalents that only make one screen pass.
- Build generic behavior from Android/APK/DEX/resource specs so unrelated
  apps benefit from the same work.
- Keep code atomic. One module owns one job, one function has one clear
  responsibility. Unix philosophy: do one thing well.
- Keep code readable and direct. Avoid clever one-liners, hidden control
  flow, and smart shortcuts that save lines but obscure behavior.
- Missing support must be visible, with the exact method or primitive and
  execution context. At guest call boundaries, best-effort execution logs a
  FIXME and substitutes void, numeric zero, or null according to the declared
  return type. These results are diagnostic substitutions, not Android behavior.
  Guest exceptions, invalid data, and execution failures remain failures.
- Do not add broad fallback stacks to hide missing core behavior. Strengthen
  the core path instead.
    - A builtin that cannot do the requested work must return None or an
      explicit missing-primitive error. The centralized call boundary owns
      diagnostic continuation; do not distribute fake-success stubs across APIs.
      (A popup builtin once returned success after failing to build its
      overlay: the menu silently never appeared.)
    - Missing-primitive reports are a development work queue. A best-effort
      run is incomplete and cannot establish compatibility.
- Preserve working real paths during refactors. Do not replace implemented
  behavior with placeholders or approximations.
- We are not making a virtual machine. This is an execution/translation
  layer, like Wine is for exe, but against the Android spec.
- Do not expand sideways into every Android API just because it exists.
    - No new framework API unless a benchmark app reaches it in a real
      run/profile/trace.
    - Architecture/runtime semantics work (interpreter correctness, dispatch,
      resource parsing, scheduling, rendering core) is always fair game.
- Do not optimize before measuring (see "Performance and measurement").

Comments are citations
----------------------

There is exactly one kind of comment in this codebase: a citation. Nothing
else is a comment. A comment that is not a `// cite: ...` line is a bug —
delete it. The code itself is the comment; prose rots, encodes assumptions,
and adds nothing a reader of the code cannot see.

The only permitted comment form:

    // cite: <repo> <FileName>#<anchor>[, <anchor>...]

- <repo> is a directory under ~/Dev/android-ref (androidx, frameworks-base,
  libcore, art, dalvik, bionic, frameworks-native, cts).
- <FileName> is the bare file name; <anchor> is a method, field, constant,
  or attribute name inside it. Every part must be findable with rg inside
  the reference checkout. No line numbers — they rot when the pin moves.
- One line per cited file; multiple anchors comma-separated; multiple files
  as consecutive cite lines. Examples:
      // cite: androidx AppCompatDelegateImpl.java#createSubDecor
      // cite: frameworks-base themes.xml#windowActionBar, windowNoTitle
      // cite: libcore String.java#indexOf

A cite is a `// cite:` line and nothing more. It carries no prose after it,
no trailing text on the same line, no parenthetical gloss, no explanation of
what the source says. If the reader needs to know what the source says, they
open the source.

Banned everywhere, with no exceptions — in src/ and in tests:

- explanatory or descriptive comments ("this handles...", "we do X because...")
- TODO / FIXME / NOTE / HACK / XXX comments (open items go in todo.txt)
- doc comments (`///`, `//!`)
- commented-out code
- narration of what the next line does
- section banners or separators

The test that this rule holds: `rg '//' src` returns only `// cite:` lines
and URLs inside string literals — nothing else. A citation is mandatory at
every decision point that mirrors framework behavior; a non-cite comment is
never allowed. If code needs prose to be understood, rewrite the code.

Source of truth: Android is open source
----------------------------------------

Android behavior is never guessed, remembered, or inferred. It is read.

- A pinned reference checkout lives at ~/Dev/android-ref (fetch or refresh it
  with scripts/fetch_android_ref.sh):
    - frameworks-base/ — platform framework (PhoneWindow, View, Activity),
      core/res where every theme/attr default lives, and libs/androidfw
      (ResourceTypes.h is the canonical resources.arsc / binary XML spec).
    - androidx/ — appcompat, activity, fragment, core, compose. Apps ship
      these as libraries; most visible UI behavior (action bars, menus,
      dialogs) lives HERE, not in the platform.
    - libcore/ — the core Java library (java.lang, java.util, java.io).
      Builtins for String, collections, IO, etc. mirror THIS, not memory of
      the JDK.
    - art/ — the real DEX interpreter and libdexfile. The reference for
      instruction semantics, verifier rules, and dex format edge cases.
    - dalvik/ — opcode tables (opcode-gen) and legacy runtime docs.
    - bionic/ — libc and the dynamic linker, for the Bionic-compatible
      native runtime.
    - frameworks-native/ — libs/input: native MotionEvent/KeyEvent
      semantics.
    - cts/ — Compatibility Test Suite subsets (view, widget, text,
      graphics, content): the first place to look for test oracles.
- Before implementing or changing any framework contract, read the real
  implementation in the reference checkout and write down its actual decision
  inputs: which theme attributes, flags, defaults, and ordering it uses.
- Any code that models a framework contract must cite the source it mirrors
  at the decision point, using the citation comment format (see "Comments
  are citations"). Framework-shaped code with no citation is a defect to
  flag in review.
- If you cannot find the source for a behavior, do not implement it. Return
  an explicit missing-primitive error and record what you searched for.
- Never infer a framework contract from what one app appears to need.
  Correlation inside one APK is not a contract; read the source and model the
  real decision inputs, not the coincidence one screen exhibits.

Truth oracles for tests
-----------------------

- Every contract test must state where its expected value came from: an
  AOSP/AndroidX source location, a CTS case, or a recorded run on real
  Android. Put the origin in the test name or a comment.
- A green test with no stated origin proves nothing about correctness. It
  only freezes current behavior — including current bugs.
- When testing a framework API, check the local CTS checkout for an existing
  case to adapt before inventing expectations.

UI truth: source first, real Android as last-resort referee
------------------------------------------------------------

- Source and CTS settle almost everything. Do not reach for a device or
  emulator for runtime semantics, builtins, or event dispatch — read the
  code in ~/Dev/android-ref instead.
- The referee exists only for visual questions that source reading cannot
  settle (exact layout results, font metrics, "what does this screen really
  look like"). Never fill that gap with intuition: capture what real Android
  does ONCE (screenshot + uiautomator view-hierarchy dump), record it, and
  work offline from the capture. A real device over adb or the emulator both
  work; the emulator may stay running across capture sessions.
- Captures live under tests/goldens/<package>/ with a note of device/API
  level and the interaction that produced them. Capture lazily when a
  dispute arises, not upfront.
- Rule adopted from Wine's conformance suite: an assertion that disagrees
  with real Android is a wrong assertion, not an Android bug.

Compatibility workflow
----------------------

1. Run the APK until it reaches missing behavior.
2. Identify the Android primitive that is missing.
3. Read the AOSP/AndroidX implementation of that primitive in the reference
   checkout. Write down its decision inputs before writing any code.
4. Implement the primitive in the correct layer, citing the source.
5. Add a contract test with a stated origin.
6. Keep unsupported behavior explicit if the primitive is not implemented
   yet.
7. App-specific notes are investigation notes only, never architecture.

Benchmark apps
--------------

Use these as regression benchmarks, never as sources of app-specific fixes.
Do not regress them.

- Open Calculator:  cargo run -- /Users/soham/com.darkempire78.opencalculator_53.apk
- Meditation Timer: cargo run -- /Users/soham/com.exner.tools.meditationtimer_10.apk
  (Compose-based; exercises the Compose path.)

Testing and automation harness
------------------------------

Use fresh `WINEDROID_PREFIX=/private/tmp/...` prefixes for clean app data
when validating behavior (state lives directly under that directory). Use the
real user prefix (`~/.winedroid/default`, used when WINEDROID_PREFIX is unset)
only when checking persisted local state.

Environment variables the runtime honors:

- WINEDROID_STRICT=1                  stop on missing method support; unset
                                       by default for diagnostic continuation
- WINEDROID_PREFIX=<dir>               prefix root for installs and app data
                                       (default: ~/.winedroid/default)
- WINEDROID_MISSING_PRIMITIVE_LOG=<path>  write every recorded missing
                                       primitive (with hit counts and first
                                       error detail) at exit; a summary also
                                       prints on stdout whenever any were hit
- WINEDROID_HEADLESS=1                 run without any host window; the full
                                       automation harness (script, autoclick,
                                       screenshot, dumps, autoclose) works
                                       unchanged
- WINEDROID_AUTOMATION_SCRIPT=<file>   timed input driver (see below)
- WINEDROID_AUTOMATION_INTERACTIVE=1   allow real mouse/keyboard input on the
                                       window while automation (script or
                                       autoclick targets) is active; without
                                       it such input is ignored so stray
                                       clicks cannot corrupt a run
- WINEDROID_AUTOMATION_LOG=<csv>       per-step latency log
- WINEDROID_WINDOW_AUTOCLOSE_MS=<ms>   close the window after N ms
- WINEDROID_WINDOW_AUTOCLICK_TARGETS=  point:x:y,view:<resource-id>,... clicks
- WINEDROID_TRACE=ui,click,bounds,render   runtime trace spans
- WINEDROID_DEX_TRACE=1                per-instruction DEX trace
- WINEDROID_RENDER_DUMP_PNG=<path>     dump the final frame as a PNG
- WINEDROID_UI_DUMP=<path>             dump the surface as a readable text
                                       tree (nodes, overlays, dialogs) with
                                       top-origin bounds and clickable flags;
                                       prefer this over reading screenshots

Automation script format: one step per line, "<delay_ms> <verb> [args]",
coordinates are top-origin screen px. Verbs that run today: tap (see forms
below; optional trailing hold_ms for long-press on runtime canvas targets),
scroll <delta_y> [X:Y] (positive scrolls down; point
defaults to the surface center), swipe X1:Y1 X2:Y2 <duration_ms> [steps]
(dispatches a real DOWN/MOVE/UP MotionEvent stream to the runtime view root),
rotary <axis_scroll> (one ACTION_SCROLL MotionEvent from SOURCE_ROTARY_ENCODER
with that AXIS_SCROLL value, delivered down the focus chain; negative scrolls
forward), key, text, waitidle,
waittext, assert (text/notext/view/display), screenshot, dump. Tap forms:

    tap X:Y [hold_ms]              raw coordinates
    tap text "7" [hold_ms]         topmost node whose text matches exactly
    tap id sevenButton [hold_ms]   resource id: bare name, pkg:id/name, or numeric

Prefer tap text / tap id in checked-in scripts — they survive layout changes
and run unchanged on real Android via scripts/emu_journey.py. The dump verb
takes "ui [path]" for a uiautomator-shaped XML view hierarchy (to stdout if
no path given; same schema as `adb shell uiautomator dump`, resource ids
resolved to pkg:id/name), or "bounds"/"surface" for trace-style output.
WINEDROID_UI_DUMP writes the same XML. Prefer "dump ui" over a
screenshot when you only need to know what is on screen and where. Example
/private/tmp/calc.auto:

    0    waittext "AC" 8000
    300  tap text "7"
    300  tap text "+"
    300  tap text "8"
    300  tap text "="
    0    waitidle 3000
    0    assert text "15"
    0    screenshot /private/tmp/calc.png

    env WINEDROID_PREFIX=/private/tmp/winedroid-auto WINEDROID_AUTOMATION_SCRIPT=/private/tmp/calc.auto WINEDROID_AUTOMATION_LOG=/private/tmp/calc.csv WINEDROID_WINDOW_AUTOCLOSE_MS=2000 target/release/winedroid /Users/soham/com.darkempire78.opencalculator_53.apk

Recipes (these use target/debug because they are for behavior/trace
debugging; swap in target/release and drop trace flags when measuring speed):

Meditation Timer traced launch with render dump:

    env WINEDROID_PREFIX=/private/tmp/winedroid-med-launch WINEDROID_TRACE=ui,click,bounds WINEDROID_WINDOW_AUTOCLOSE_MS=12000 WINEDROID_RENDER_DUMP_PNG=/private/tmp/winedroid-med-launch.png target/debug/winedroid /Users/soham/com.exner.tools.meditationtimer_10.apk

Meditation Timer Add -> Create submit -> row tap -> Start/timer probe:

    env WINEDROID_PREFIX=/private/tmp/winedroid-med-flow WINEDROID_TRACE=ui,click,bounds WINEDROID_WINDOW_AUTOCLOSE_MS=20000 WINEDROID_WINDOW_AUTOCLICK_TARGETS=point:388:725,point:350:724,point:90:150,point:350:724 WINEDROID_RENDER_DUMP_PNG=/private/tmp/winedroid-med-flow.png target/debug/winedroid /Users/soham/com.exner.tools.meditationtimer_10.apk

Meditation Timer DEX trace for the next missing primitive:

    env WINEDROID_PREFIX=/private/tmp/winedroid-med-dex WINEDROID_DEX_TRACE=1 WINEDROID_TRACE=ui,click WINEDROID_WINDOW_AUTOCLOSE_MS=20000 WINEDROID_WINDOW_AUTOCLICK_TARGETS=point:388:725,point:350:724,point:90:150,point:350:724 target/debug/winedroid /Users/soham/com.exner.tools.meditationtimer_10.apk

Open Calculator traced launch with render dump:

    env WINEDROID_PREFIX=/private/tmp/winedroid-opencalc-launch WINEDROID_TRACE=ui,click,bounds WINEDROID_WINDOW_AUTOCLOSE_MS=12000 WINEDROID_RENDER_DUMP_PNG=/private/tmp/winedroid-opencalc-launch.png target/debug/winedroid /Users/soham/com.darkempire78.opencalculator_53.apk

Open Calculator render-trace for per-refresh span timings (summarize cost):

    env WINEDROID_PREFIX=/private/tmp/winedroid-opencalc-render WINEDROID_TRACE=render,ui WINEDROID_WINDOW_AUTOCLOSE_MS=4000 WINEDROID_WINDOW_AUTOCLICK_TARGETS=point:159:406 target/release/winedroid /Users/soham/com.darkempire78.opencalculator_53.apk

Open Calculator DEX trace for click/settings failures:

    env WINEDROID_PREFIX=/private/tmp/winedroid-opencalc-dex WINEDROID_DEX_TRACE=1 WINEDROID_TRACE=ui,click WINEDROID_WINDOW_AUTOCLOSE_MS=18000 WINEDROID_WINDOW_AUTOCLICK_TARGETS=view:2131296591,view:2131296357 target/debug/winedroid /Users/soham/com.darkempire78.opencalculator_53.apk

Journeys, goldens, and the real-Android oracle
----------------------------------------------

One journey file (the automation script format above) drives both backends:

- winedroid runs it via WINEDROID_AUTOMATION_SCRIPT.
- Real Android runs it via `python3 scripts/emu_journey.py --artifacts <dir>
  <journey>` (adb-based; resolves tap text / tap id from uiautomator dumps).
- Checked-in journeys live in tests/journeys/<package>/<name>.auto and use
  only portable steps (tap text / tap id, never raw coordinates) with
  relative `dump ui <name>.xml` / `screenshot <name>.png` checkpoints.

`scripts/capture_oracle.sh <apk> <package> <journey> <out_dir>` runs the
journey on both backends (boots the emulator headless if needed, kills it
after if it started it) and compares every checkpoint with
`scripts/diff_ui_dump.py` — the parity report: missing/extra nodes, text,
role, and state diffs fail it; bounds compare as screen fractions within a
tolerance. Divergences it reports are the missing-primitive work queue.

Frozen goldens live in tests/goldens/<package>/<journey>/: winedroid's own
checkpoint XMLs (exact-diffed by validate.sh on every run — the regression
tripwire) plus *.reference.xml captured on real Android and a NOTES.md with
the device/API level and known divergence. When a projection change is
intended, rerun the journey, review the golden diff, and copy the new dumps
deliberately; never hand-edit a golden.

The differ itself is covered by scripts/test_diff_ui_dump.py (validate.sh
runs it): a differ that silently passes is worse than none, so any change to
diff_ui_dump.py must keep that self-test green. Tap/assert misses are hard
failures on both backends — a `tap text`/`tap id` matching no node, or a
failing assert, exits nonzero rather than silently no-opping.

Performance and measurement
---------------------------

- MEASURE FIRST (automation latency log / WINEDROID_TRACE spans / macOS
  `sample`), then change. Two planned refactors were avoided this way (a
  3000-site DexValue Rc change; a settle-pass change) — both targeted things
  the profile showed were NOT the bottleneck.
- DEBUG PANICS ON INTEGER OVERFLOW, RELEASE WRAPS SILENTLY. If release
  misbehaves with no error output (something just doesn't render or
  compute), reproduce in a debug build first; the panic points at the line.
- DEBUG IS ~100x SLOWER THAN RELEASE for the interpreter. The old
  "1.5s/frame" was debug overhead; release runs the same flow at ~7ms/pump
  and ~15ms/settle (inside the 60fps budget). opt-level=1 makes debug
  bearable (~5x), but use target/release for any real perf claim.
- Per-tap cost on a real click is NOT redundant settle; it is
  summarize_activity_layouts (~40-60ms, and it re-opens the APK each
  refresh) + the onClick DEX execution. Those are the two real levers.
- Known debt: the instance_fields store is OVERLOADED — collections.rs and
  sqlite.rs stash their own internals in the same map via synthetic string
  keys ("{class}.a:", bind-index names). Must disentangle before any
  numbered-slot field storage. Broadly, runtime.rs is full of
  HashMap<String,...> keyed by map_identity_key (format!("object:{id}")) —
  string keys are the pervasive interpreter cost.
- Deferred until behavior and module boundaries are stable: runtime perf
  tuning, renderer caching, interpreter hot-path optimization, speculative
  native shortcuts, broad fallback systems.

Validation before commit
------------------------

- Run focused tests for changed runtime behavior first; they localize the
  failure.
- Then run `scripts/validate.sh` — the canonical gate. It runs
  `cargo fmt --check`, the full `cargo test --offline`, and both benchmark
  smokes headless:
    - Meditation Timer: tap Add, wait for the Create form.
    - Open Calculator: no title text ever appears (notext "Calculator",
      notext "com.darkempire78"), the top-right menu opens with its items,
      an outside tap dismisses it, 7+8=15, and the settings view opens.
- The same script is the merge gate for parallel work (see "Parallel
  agents"); if it passes locally it means the same thing everywhere.

Task tracking
-------------

- Work items live in todo.txt. Mark them off as you go.
- Add new items only when necessary for the current APK goal. Watch for the
  spiral of filling points by adding points and drifting away from the APK
  you are trying to make work.
