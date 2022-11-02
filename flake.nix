{
  description = "package set for rivos hubris builds";
  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";

  inputs.flake-utils.url = "github:numtide/flake-utils";

  inputs.rust-overlay.url = "github:oxalica/rust-overlay";
  inputs.rust-overlay.inputs.nixpkgs.follows = "nixpkgs";
  inputs.rust-overlay.inputs.flake-utils.follows = "flake-utils";

  inputs.humilityflake.url = "github:rivosinc/humility";
  inputs.humilityflake.inputs.rust-overlay.follows = "rust-overlay";
  inputs.humilityflake.inputs.nixpkgs.follows = "nixpkgs";
  inputs.humilityflake.inputs.flake-utils.follows = "flake-utils";

  outputs = {
    self,
    nixpkgs,
    flake-utils,
    rust-overlay,
    humilityflake,
  }:
    flake-utils.lib.eachDefaultSystem (system: let
      overlays = [(import rust-overlay)];

      pkgs = import nixpkgs {
        inherit system overlays;
      };

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
        };
      };

      rust = pkgs.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml;

      #since hubris looks for 64bit named binaries
      binutils32 = pkgs.binutils-unwrapped.override {
        stdenv = riscv32-unknown-none-elf-stdenv;
      };
      binutils64 = pkgs.binutils-unwrapped.override {
        stdenv = riscv64-unknown-none-elf-stdenv;
      };

      hubris = (
        {
          app,
          toml,
          doCheck ? false,
          sixtyfour ? false,
        }:
          pkgs.callPackage ./nix/hubris.nix {
            inherit binutils64 app toml doCheck sixtyfour;
            cargo = rust;
            rustc = rust;
            src = pkgs.lib.cleanSource ./.;
            version = "beta";
          }
      );

      hubris-test-suite-runner = (
        {
          hubris,
          app,
          sixtyfour ? false,
        }:
          pkgs.callPackage ./nix/qemu-test-suite.nix {
            inherit hubris app sixtyfour;
            humility = humilityflake.packages.${system}.humility;
          }
      );

      target = "demo-hifive-inventor";
    in {
      packages = flake-utils.lib.flattenTree {
        demo-hifive1-revb = hubris {
          app = "demo-hifive1-revb";
          toml = "app/demo-hifive1-revb/app.toml";
        };
        demo-hifive-inventor = hubris {
          app = "demo-hifive-inventor";
          toml = "app/demo-hifive-inventor/app.toml";
        };
        tests-hifive-inventor = hubris {
          app = "tests-hifive-inventor";
          toml = "test/tests-hifive-inventor/app.toml";
        };
      };

      devShells.default = pkgs.mkShell {
        shellHook = ''
          export HUMILITY_ARCHIVE=$(pwd)/target/${target}/dist/default/build-${target}.zip
          export CARGO_HOME=$HOME/.cargo
        '';

        nativeBuildInputs = with pkgs; [
          rust
          qemu
          openocd
          binutils32
          binutils64
          humilityflake.packages.${system}.humility
        ];
      };

      checks = {
        # build checks
        demo-hifive1-revb = self.packages.${system}.demo-hifive-inventor.override {doCheck = true;};
        demo-hifive-inventor = self.packages.${system}.demo-hifive-inventor.override {doCheck = true;};
        tests-hifive-inventor = self.packages.${system}.tests-hifive-inventor.override {doCheck = true;};
      };

      formatter = nixpkgs.legacyPackages.${system}.alejandra;
    });
}
