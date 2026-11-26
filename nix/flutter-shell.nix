{
  pkgs ? import <nixpkgs> { },
  extraPackages ? (_: [ ]),
}:

# FVM downloads SDK tools that expect the standard Linux ELF loader and paths.
(pkgs.buildFHSEnv {
  name = "remya-flutter";
  includeClosures = true;
  extraOutputsToInstall = [ "dev" ];
  # CMake also probes the private pkg-config dependencies of GTK and GLib.
  targetPkgs = pkgs:
    (with pkgs; [
      clang
      cmake
      curl
      fontconfig
      fvm
      gcc
      gitMinimal
      gtk3
      inter
      libdatrie.dev
      libepoxy
      libselinux.dev
      libsepol.dev
      libsysprof-capture
      libthai.dev
      libxdmcp.dev
      libxkbcommon
      ninja
      pcre2
      pkg-config
      python3
      rustup
      systemdMinimal.dev
      unzip
      util-linuxMinimal.dev
      which
    ])
    ++ extraPackages pkgs;
  profile = ''
    export PKG_CONFIG_PATH="/usr/share/pkgconfig:$PKG_CONFIG_PATH"
  '';
}).env
