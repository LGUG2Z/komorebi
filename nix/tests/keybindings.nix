# `keybindings-fixtures`: every key binding file of the fixture corpus
# (komorebi-hotkeys/tests/fixtures) through `komorebic hotkeys compile --mode auto --dry-run`,
# the command the NixOS module runs, compared with `keybindings.expected`: the static shortcuts of
# each file, the bindings left to the engine, and the diagnostics, as the build log of the module
# shows them. After a change that means to change them, write the file again with the loop below,
# run in the fixtures directory, and read the difference. The sample files of the user guide,
# docs/*.sample, are the corpus's case `sample`, which this checks too.
{
  pkgs,
  # a package with bin/komorebic
  komorebic,
}:
let
  inherit (pkgs) lib;
in
pkgs.runCommand "keybindings-fixtures"
  {
    fixtures = ../../komorebi-hotkeys/tests/fixtures;
    expected = ./keybindings.expected;
    samples = lib.fileset.toSource {
      root = ../../docs;
      fileset = lib.fileset.fileFilter (file: file.hasExt "sample") ../../docs;
    };
  }
  ''
    for format in skhdrc whkdrc sxhkdrc; do
      diff -u $fixtures/sample.$format $samples/$format.sample
    done
    cd $fixtures
    for file in $(ls | sort); do
      echo "== $file"
      ${komorebic}/bin/komorebic hotkeys compile --mode auto --dry-run "$file" --output shortcuts 2>&1
      echo
    done > $TMPDIR/got
    diff -u $expected $TMPDIR/got
    echo "$(ls $fixtures | wc -l) files plan as expected"
    touch $out
  ''
