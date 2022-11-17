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

  inputs.qemuflake.url = "git+https://github.com/rivosinc/qemu?submodules=1&ref=dev/drew/nix";
  inputs.qemuflake.inputs.nixpkgs.follows = "nixpkgs";

  inputs.pre-commit-hooks.url = "github:cachix/pre-commit-hooks.nix";
  inputs.pre-commit-hooks.inputs.flake-utils.follows = "flake-utils";
  inputs.pre-commit-hooks.inputs.nixpkgs.follows = "nixpkgs";

  outputs = {
    self,
    nixpkgs,
    flake-utils,
    rust-overlay,
    humilityflake,
    qemuflake,
    pre-commit-hooks,
  }:
    flake-utils.lib.eachDefaultSystem (system: let
      overlays = [(import rust-overlay) qemuflake.overlays.default];

      pkgs = import nixpkgs {
        inherit system overlays;
      };

      # pull in the appropriate rust toolchain
      rust = pkgs.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml;
      # pull in correct binutils
      binutils = (pkgs.callPackage ./nix/binutils.nix {}).binutils;
      # this exports the correct build archive for using humility with this app
      target = "demo-hifive-inventor";

      # build xtask seperatly to save time when building multiple apps
      xtask = pkgs.callPackage ./nix/xtask.nix {
        inherit binutils;
        cargo = rust;
        rustc = rust;
        src = pkgs.lib.cleanSource ./.;
      };

      # actual build function for hubris apps
      hubris = (
        {
          app,
          toml,
          doCheck ? false,
        }:
          pkgs.callPackage ./nix/hubris.nix {
            inherit xtask app toml doCheck;
            cargo = rust;
            rustc = rust;
            src = pkgs.lib.cleanSource ./.;
          }
      );

      # build function to generate qemu test suite runners for different test apps
      hubris-test-suite-runner = (
        {hubris}:
          pkgs.callPackage ./nix/qemu-test-suite.nix {
            inherit hubris;
            humility = humilityflake.packages.${system}.humility;
          }
      );

      # automatically setup precommit checks for cargo fmt
      cargo-pre-commit-checks = pre-commit-hooks.lib.${system}.run {
        src = pkgs.lib.cleanSource ./.;
        hooks = {
          cargofmt = {
            enable = true;
            name = "cargo-fmt";
            entry = "${rust}/bin/cargo fmt --check";
            files = "\\.rs$";
            pass_filenames = true;
          };
        };
      };
    in {
      ## Here is where you list all of the apps that nix should/can build
      packages = flake-utils.lib.flattenTree {
        demo-hifive1-revb = hubris {
          app = "demo-hifive1-revb";
          toml = "app/demo-hifive1-revb/app.toml";
          doCheck = true;
        };
        demo-hifive-inventor = hubris {
          app = "demo-hifive-inventor";
          toml = "app/demo-hifive-inventor/app.toml";
          doCheck = true;
        };
        tests-hifive-inventor = hubris {
          app = "tests-hifive-inventor";
          toml = "test/tests-hifive-inventor/app.toml";
          # don't do check, test suite is NOT clippy clean
        };
        tests-hifive-inventor-runner = hubris-test-suite-runner {hubris = self.packages.${system}.tests-hifive-inventor;};
      };

      devShells.default = pkgs.mkShell {
        shellHook = ''
          # this enables running humility without specify the archive everytime
          export HUMILITY_ARCHIVE=$(pwd)/target/${target}/dist/default/build-${target}.zip
          # this is expected by xtask
          export CARGO_HOME=$HOME/.cargo
          ${cargo-pre-commit-checks.shellHook}
        '';

        nativeBuildInputs = with pkgs; [
          rust
          binutils
          git
          qemu
          openocd
          gdb
          humilityflake.packages.${system}.humility
        ];
      };

      # build all packages for check
      checks =
        self.packages.${system};

      formatter = nixpkgs.legacyPackages.${system}.alejandra;
    });
}
