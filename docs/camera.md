# Camera

Camera is under Bluetooth & devices (`cameras`). The ordinary-user Rust service
uses pipewire-rs 0.10.1 (MIT) and dynamically linked libpipewire to enumerate
Video/Source nodes, their PropInfo ranges/defaults and current Props. Parameter
writes validate the current node name and serial, capability, range and step,
then read back the requested values. No V4L2 tools or privileged processes run.

Only brightness, contrast, saturation and sharpness are exposed. Unsupported
controls are hidden. Reset restores only writable controls among these four.
Device selection is local to the page and does not change a system default.
Devices refresh every two seconds while the page is open, and through the
service's normal 30-second refresh. A failed read retains the last readout and
disables controls. Operation failures remain visible after automatic refreshes.

The service records actual values on first use in cameras.toml next to its main
configuration, using atomic writes. Startup/page reads retain saved and actual
values and report per-camera conflicts. Resolve adopts actual values or applies
saved values with validation/readback. Missing devices retain saved preferences;
they are never automatically restored. Failed writes or persistence retain the
previous saved values. Only present, supported controls participate in comparison.

## Preview

Preview starts only after an explicit click, in the disposable GUI process.
Rust GStreamer 0.25 bindings (MIT/Apache-2.0) dynamically link system GStreamer
1.28+, using pipewiresrc, videoconvert, videoscale, videorate and appsink.
PipeWire and WirePlumber must be running. Preview negotiates the device's reported
frame rate and delivers a letterboxed 640x480 RGBA image at 15 FPS, with a single
latest-frame queue. The opt-in `remya_camera/preview` feature exposes plain Rust
start, next-frame and stop functions in `remya_camera::preview`; it is disabled
by default. Core contains no Flutter bridge. Settings depends on this feature
through a normal Cargo path dependency and owns the FRB wrapper and Dart bindings.
Frames are neither persisted nor sent over D-Bus. Missing plugins and capture errors
are reported. Stop, selection changes, page disposal and backgrounding release
capture. A stopped or disconnected preview must be explicitly restarted.

The four controls and reset sit to the right of preview on wide windows and below
it on narrow windows. Labels accompany Material icons. No network camera setup,
recording, photographs or application permission management is provided.

## Verification

The GUI split cleanup (2026-11-14) passes Core workspace formatting, Clippy and
tests, plus Clippy for all camera targets with `--features preview`. Settings'
FRB 2.13.0 wrapper passes build, Clippy, its hardware-free invalid-handle test,
and a release shared-library link. This cleanup did not repeat live capture or
parameter writes and did not install either application.

Rust unit tests cover initialization, validation, stale identity, unsupported
fields, both conflict resolutions, reset, failed detection/readback/persistence,
and retention after unplugging. Flutter camera tests cover request identity,
responsive layout, opt-in capture and disposal with late completion. A delayed
write/readback regression verifies sliders retain their released value until
the operation finishes, without briefly displaying the previous value.

The explicit hardware examples `verify` and `preview` require attached cameras:
`cargo run -p remya_camera --example preview --features preview` enables capture.
`verify` writes and restores each of the four adjustable parameters, checking
readback; `preview` obtains and releases a frame for each detected node.
On this workstation both the HP normal and IR sources produced 640x480 frames;
the normal source's four parameter writes/restoration passed. IR has no exposed
image adjustment controls. No unplug/replug operation was performed on hardware.

Private-bus smoke tests (`python test/camera_smoke.py debug|release`) verify
CLI/D-Bus state parity, first-use initialization without cameras, read-only
inventory, rejected stale/malformed operations, and deleted-page rejection.
