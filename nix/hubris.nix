{
  lib,
  stdenv,
  rustPlatform,
  tree,
  binutils64,
  qemu,
  cargo,
  rustc,
  toml ? null,
  app ? null,
  rev ? "dirty",
  version,
  src,
  doCheck ? false,
}:
# host and build platform are linux
# target platform should be riscv32
stdenv.mkDerivation rec {
  inherit src version doCheck;
  name = "hubris";

  CARGO_HOME = src;
  cargoDeps = rustPlatform.importCargoLock {
    lockFile = src + /Cargo.lock;
    outputHashes = {
      "gateway-messages-0.1.0" = "sha256-7vQTj/j5iQTqqQjgXSM7G2VnWLhXGR/AuCpo2LU1/uw";
      "hif-0.3.1" = "sha256-o3r1akaSARfqIzuP86SJc6/s0b2PIkaZENjYO3DPAUo";
      "hubpack-0.1.0" = "sha256-Q5wwLAWbwCVociZBeTy9SeCF0MmKmcG0C3LQAtdw/Mc";
      "idol-0.2.0" = "sha256-kkaVPgr7kp+xif4Me6DvNwN6QLKJYAkT6mLD4pMH1Aw";
      "lpc55_sign-0.1.0" = "sha256-To4+Dn/BcvpBwQRNaI+wn68G4hqpCrmO/2CLqCcdWTE";
      "ordered-toml-0.1.0" = "sha256-hJjyF9bXt5CfemV5/ogPViaNHZsINrQkZG45Ta65Qm4";
      "pmbus-0.1.0" = "sha256-rKEbuWUp88PJK+gP6dmaIeeBNXzjclNpwE5kibViYQQ";
      "riscv-0.8.1" = "sha256-icLG4b/jpn4thGoeXHHBAEqF6FLV8u/8N0KLOgeQcO0";
      "riscv-pseudo-atomics-0.1.0" = "sha256-QuChdKbw1TTy8W+mr3gF8yDfwWcUxmAzT3j5A5gamdk";
      "riscv-semihosting-0.0.1" = "sha256-JzQyv2/4F1FBclFZUsIOJ1TREaBIKQp/TOZhBSHyQGY";
      "salty-0.2.0" = "sha256-8RnvQ+Ch4RijmOhWNQZh7PmFlZGUfyzbeRvSKWqsbJU";
      "spd-0.1.0" = "sha256-X6XUx+huQp77XF5EZDYYqRqaHsdDSbDMK8qcuSGob3E";
      "stm32g0-0.15.1" = "sha256-mWY3CU0bUdlBKZKAoyjGVSdT3KVLgPHb4Jjb/JvPXEA";
      "tlvc-0.1.0" = "sha256-uHPPyc3Ns5L1/EFNCzH8eBEoqLlJoqguZxwNCNxfM6Q";
      "vsc7448-pac-0.1.0" = "sha256-otNLdfGIzuyu03wEb7tzhZVVMdS0of2sU/AKSNSsoho";
    };
  };

  nativeBuildInputs = [
    rustPlatform.cargoSetupHook
    tree
    cargo
    rustc
  ];

  depsBuildTarget = [
    binutils64
  ];

  patchPhase = ''
    substituteInPlace build/xtask/src/dist.rs --replace 'get_git_status()?' '("${rev}", false)'
  '';

  buildPhase = ''
    ${cargo}/bin/cargo --offline --frozen xtask dist ${toml}
  '';

  installPhase = ''
    mkdir -p $out
    cp target/${app}/ $out/ -a
  '';

  dontFixup = true;

  checkPhase = ''
    pushd ${src}
    ${cargo}/bin/cargo --offline --frozen fmt --check --all
    popd
  '';

  meta = with lib; {
    description = "kernel";
    platforms = platforms.all;
  };
}
