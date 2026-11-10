# The source of a build of one platform, cut from the one Cargo workspace of this repository.
#
#   import ./workspace-src.nix { pkgs; craneLib; repoRoot; platform = "windows" | "linux" | "macos" | "recorder" | "nixgen" | "docgen"; }
#
# Cargo has to load every member of the workspace (the root manifest lists the members of all three
# platforms), but a build of one platform needs the sources of that platform's crates only. The result:
#
#   real      the filtered source: the members of the platform, the root manifest and lock, the
#             platform's files of the crates the platforms share, and the data files they read
#   src       `real` plus, for every other member, its own manifest with an empty library and a `main`
#             that does nothing (a stub: cargo refuses a workspace with a member that is not there, and
#             a lock file that has packages the workspace does not have fails under --locked)
#   members   the members of the platform; stubbed the others; stubbedPackages their package names (for
#             `cargo deny --exclude`)
#
# The platform "recorder" is the source of komorebi-recorder's own builds on macOS and Linux: the recorder
# indexes the log statements of the daemons of all three platforms (komorebi-recorder/build.rs), so it
# has their sources, whole, and nothing else; the Windows build has the recorder as one of its members.
#
# The platforms "nixgen" and "docgen" are the sources of schemars-nixgen's and schemars-docgen's own builds:
# the tool and nothing else. Each is a stub in every other platform, so that an edit of it changes none of
# their derivations.
#
# An edit under one platform therefore changes nothing in the derivations of the others, except where
# the edit is in a crate they share or in the root manifest, lock or toolchain files.
{
  pkgs,
  craneLib,
  repoRoot,
  platform,
}:
let
  inherit (pkgs) lib;
  root = toString repoRoot;

  allMembers = (builtins.fromTOML (builtins.readFile (repoRoot + "/Cargo.toml"))).workspace.members;

  # the crates of the root workspace that the KDE packages depend on, by path (nix/linux/root-crates.json,
  # kept equal to `cargo tree` by tools/kde-closure-check); komorebi-bar and komorebi-bar-core (the egui
  # bar) are not in it, so an edit of the bar does not rebuild the KDE packages
  linuxShared = builtins.fromJSON (builtins.readFile ./linux/root-crates.json) ++ [
    "komorebi"
  ];

  # the crates only one platform builds; the Windows set also has the e2e guests and the Windows-only
  # binaries, which the other platforms never build
  linuxOnly = [
    "crates/komorebi-linux"
    "crates/komorebi-hotkeys"
  ];
  macosOnly = [ "crates/komorebi-macos" ];

  # crates that no Linux package depends on, but whose tests need no macOS and so run in `linux-nextest`:
  # the FFI-free part of the macOS daemon. They are not in linux/root-crates.json (that list is the
  # `cargo tree` of the Linux packages, which tools/kde-closure-check keeps equal), only members of the
  # Linux source.
  linuxNeutralTests = [ "crates/komorebi-macos-core" ];
  windowsOnly = [
    "crates/komorebi-windows"
    "komorebi-gui"
    "komorebic-no-console"
    "komorebi-shortcuts"
  ];

  # the development tools: schemars-nixgen writes the Nix options (nix/nixgen) and schemars-docgen the
  # reference pages of the docs site (nix/docgen); each is built by its own checks
  toolOnly = [
    "crates/schemars-nixgen"
    "crates/schemars-docgen"
  ];

  # the recorder and the crates whose source its build script indexes (komorebi-recorder/build.rs)
  recorderMembers = [
    "komorebi-recorder"
    "crates/komorebi-windows"
    "crates/komorebi-macos"
    "crates/komorebi-linux"
    "crates/komorebi-client"
    "crates/komorebi-layouts"
    "crates/komorebi-core"
    "crates/komorebi-transport"
  ];

  membersOf = {
    recorder = recorderMembers;
    nixgen = [ "crates/schemars-nixgen" ];
    docgen = [ "crates/schemars-docgen" ];
    # everything but the other two platforms' crates and the tool
    windows = builtins.filter (m: !(builtins.elem m (linuxOnly ++ macosOnly ++ toolOnly))) allMembers;
    linux = linuxOnly ++ linuxShared ++ linuxNeutralTests;
    # everything but the Windows and Linux crates, the tool and the recorder
    macos = builtins.filter (
      m: !(builtins.elem m (windowsOnly ++ linuxOnly ++ toolOnly ++ [ "komorebi-recorder" ]))
    ) allMembers;
  };

  members = builtins.filter (m: builtins.elem m membersOf.${platform}) allMembers;
  stubbed = builtins.filter (m: !(builtins.elem m members)) allMembers;
  stubbedPackages = map (
    m: (builtins.fromTOML (builtins.readFile (repoRoot + "/${m}/Cargo.toml"))).package.name
  ) stubbed;

  # the crates one entry point serves for every platform: each keeps its own platform's modules and
  # tests (named by prefix) and drops the other platforms'
  sharedCrates = [
    "komorebic"
    "crates/komorebi-client"
    "komorebi-bar"
    "komorebi"
  ];
  markers = {
    windows = "(src/windows(\\.rs|/.*)|tests/windows_.*)";
    linux = "(src/linux(\\.rs|/.*)|tests/linux_.*)";
    macos = "(src/macos(\\.rs|/.*)|tests/macos_.*)";
  };
  # the recorder indexes every platform's files
  otherMarkers =
    if
      builtins.elem platform [
        "recorder"
        "nixgen"
        "docgen"
      ]
    then
      [ ]
    else
      builtins.attrValues (removeAttrs markers [ platform ]);
  ofOtherPlatform =
    rel:
    builtins.any (
      crate:
      lib.hasPrefix "${crate}/" rel
      && builtins.any (
        marker: builtins.match marker (lib.removePrefix "${crate}/" rel) != null
      ) otherMarkers
    ) sharedCrates;

  # the directories of the source (a file is kept only under one of them)
  roots =
    members
    ++ lib.optional (platform == "windows") "wix"
    ++ (
      if platform == "nixgen" || platform == "docgen" then
        [ ".cargo" ]
      else if platform == "recorder" then
        [
          ".cargo"
          # the installer, which a test of the recorder checks for its binaries
          "wix"
        ]
      else
        [
          "docs"
          "schemas"
          ".cargo"
        ]
    )
    # the configurations that the config_parity test of komorebi-linux loads
    ++ lib.optional (platform == "linux") "spikes/config-parity";
  underRoot = rel: builtins.any (r: rel == r || lib.hasPrefix "${r}/" rel) roots;
  # a directory above a root (`crates` for `crates/komorebi-core`)
  aboveRoot = rel: builtins.any (r: lib.hasPrefix "${rel}/" r) roots;

  # data files that the Rust sources and tests read, outside the Cargo sources
  extra =
    rel:
    # the test inputs of every crate
    (builtins.match ".*/(tests|fixtures|golden)/.*" rel != null)
    || {
      windows =
        # the whole of docs/ (they are embedded in komorebic and the bar) except the other platforms' and
        # the developer notes; the macOS CLI pages are read by a test of komorebic-core
        (
          lib.hasPrefix "docs/" rel
          && !(builtins.any (p: lib.hasPrefix p rel) [
            "docs/linux/"
            "docs/macos/"
            "docs/dev/"
          ])
        )
        || lib.hasPrefix "docs/macos/cli/" rel
        || lib.hasPrefix "schemas/" rel
        # the icon and the guide the recorder embeds, and the installer a test of it reads
        || lib.hasPrefix "komorebi-recorder/" rel
        || lib.hasPrefix "wix/" rel;
      linux =
        # the script komorebi loads into KWin and the JSON the tests and the daemon read
        (lib.hasSuffix ".js" rel || lib.hasSuffix ".json" rel)
        && !(lib.hasPrefix "docs/" rel || lib.hasPrefix "schemas/" rel)
        # the schemas of every platform: a test of komorebi-config compares them
        || lib.hasPrefix "schemas/" rel
        # the macOS CLI pages that a test of komorebic-core compares with the commands
        || lib.hasPrefix "docs/macos/cli/" rel
        || lib.hasPrefix "spikes/config-parity/" rel
        || builtins.elem rel [
          # the example configuration and the sample key bindings that `komorebic quickstart` includes
          "docs/linux/komorebi.example.json"
          "docs/linux/skhdrc.sample"
          # the page that shows the example configuration, which a test compares with the file
          "docs/linux/example-configurations.md"
          # the example a test loads, to see that the docs show one that works
          "docs/linux/usage/wallpapers.md"
        ];
      macos =
        lib.hasSuffix ".json" rel && !(lib.hasPrefix "docs/" rel || lib.hasPrefix "schemas/" rel)
        || lib.hasPrefix "docs/macos/" rel
        || lib.hasPrefix "schemas/macos/" rel;
      # the guide and the icon the recorder embeds, and the installer
      recorder = lib.hasPrefix "komorebi-recorder/" rel || lib.hasPrefix "wix/" rel;
      nixgen = false;
      # the templates the tool embeds (include_str!)
      docgen = lib.hasPrefix "crates/schemars-docgen/src/templates/" rel;
    }
    .${platform};

  real = lib.cleanSourceWith {
    src = repoRoot;
    filter =
      path: type:
      let
        rel = lib.removePrefix "${root}/" path;
        isRootFile = !(lib.hasInfix "/" rel);
      in
      if type == "directory" then
        rel == root || aboveRoot rel || (underRoot rel && !(ofOtherPlatform rel))
      else
        # the files at the top: Cargo.toml, Cargo.lock, rust-toolchain.toml, rustfmt.toml, deny.toml
        (isRootFile && craneLib.filterCargoSources path type)
        || (
          underRoot rel && !(ofOtherPlatform rel) && ((craneLib.filterCargoSources path type) || extra rel)
        );
  };

  stub = member: ''
    mkdir -p $out/${member}/src
    cp ${
      builtins.path {
        path = repoRoot + "/${member}/Cargo.toml";
        name = "Cargo.toml";
      }
    } $out/${member}/Cargo.toml
    : > $out/${member}/src/lib.rs
    echo 'fn main() {}' > $out/${member}/src/main.rs
  '';

  src = pkgs.runCommand "komorebi-${platform}-src" { } ''
    cp -r ${real} $out
    chmod -R u+w $out
    ${lib.concatMapStrings stub stubbed}
  '';
in
{
  inherit
    real
    src
    members
    stubbed
    stubbedPackages
    ;
}
