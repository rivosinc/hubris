{
  lib,
  stdenv,
  rustPlatform,
  xtask,
  cargo,
  rustc,
  toml ? null,
  app ? null,
  src,
  doCheck ? false,
}:
# host and build platform are linux
# target platform should be riscv32
stdenv.mkDerivation rec {
  inherit src app toml doCheck;
  name = "hubris";

  # inherit all the cargo dependencies from xtask
  cargoDeps = xtask.cargoDeps;

  nativeBuildInputs = [
    rustPlatform.cargoSetupHook
    cargo
    rustc
    xtask
  ];
  
  # moves check phase before build phase.  We do not want to wait for a full build only to fail formatting...
  phases = ''
    unpackPhase checkPhase buildPhase installPhase
  '';

  buildPhase = ''
    export CARGO_HOME=$(pwd)

    # xtask uses this variable to find the hubris root, expecting to be run as `cargo xtask dist ...`.  Since we are invoking directly we need to manually set this path.
    export CARGO_MANIFEST_DIR=$(pwd)/build/xtask

    ${xtask}/bin/xtask dist ${toml}
  '';

  installPhase = ''
    mkdir -p $out
    cp target/${app}/dist/default/build-${app}.zip $out/ -a
  '';

  checkPhase = ''
    ${cargo}/bin/cargo --offline --frozen fmt --check --all
    # clippy includes `--frozen` so no need to specify
    ${xtask}/bin/xtask clippy ${toml}
  '';

  dontFixup = true;

  meta = with lib; {
    description = "kernel";
    platforms = platforms.all;
  };
}
