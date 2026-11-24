{ pkgs ? import <nixpkgs> { } }:

pkgs.mkShell {
  nativeBuildInputs = with pkgs; [
    dbus
    (lib.getBin glib)
    pkg-config
    python3
    rustup
    rustPlatform.bindgenHook
  ];

  buildInputs = with pkgs; [
    gdk-pixbuf
    glib
    libpulseaudio
    libxkbcommon
    pipewire
    sqlite
    udev
  ];

  REMYA_DBUS_SESSION_CONFIG = "${pkgs.dbus}/share/dbus-1/session.conf";

  # Keep iterative debug builds small in the VM.
  CARGO_INCREMENTAL = "0";
  CARGO_PROFILE_DEV_DEBUG = "line-tables-only";
  CARGO_PROFILE_TEST_DEBUG = "0";
}
