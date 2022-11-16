{
  pkgs,
  symlinkJoin,
}: let
  # This is a little funky since the default riscv binutils use `none` rather than `unknown`
  riscv64-unknown-none-elf-stdenv = pkgs.stdenv.override {
    targetPlatform = {
      config = "riscv64-unknown-elf";
      system = "riscv64-none";
      parsed = {
        kernel = {
          execFormat = {
            name = "unknown";
          };
          name = "none";
        };
      };
      isiOS = false;
      isAarch32 = false;
      isWindows = false;
      isMips64n64 = false;
      isMusl = false;
      isPower = false;
      isVc4 = false;
      isAvr = false;
    };
  };
  riscv32-unknown-none-elf-stdenv = pkgs.stdenv.override {
    targetPlatform = {
      config = "riscv32-unknown-elf";
      system = "riscv32-none";
      parsed = {
        kernel = {
          execFormat = {
            name = "unknown";
          };
          name = "none";
        };
      };
      isiOS = false;
      isAarch32 = false;
      isWindows = false;
      isMips64n64 = false;
      isMusl = false;
      isPower = false;
      isVc4 = false;
      isAvr = false;
    };
  };
in rec {
  #since hubris looks for 64bit named binaries
  binutils32 = pkgs.binutils-unwrapped.override {
    stdenv = riscv32-unknown-none-elf-stdenv;
  };
  binutils64 = pkgs.binutils-unwrapped.override {
    stdenv = riscv64-unknown-none-elf-stdenv;
  };
  binutils = symlinkJoin {
    name = "binutils";
    paths = [binutils32 binutils64];
  };
}
