{
  description = "pocoshelf Rust TUI file explorer";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
  };

  outputs =
    { self, nixpkgs }:
    let
      systems = [
        "aarch64-darwin"
        "x86_64-darwin"
        "aarch64-linux"
        "x86_64-linux"
      ];

      forAllSystems =
        function:
        nixpkgs.lib.genAttrs systems (
          system:
          function system (
            import nixpkgs {
              inherit system;
            }
          )
        );
    in
    {
      packages = forAllSystems (
        system:
        pkgs:
        let
          inherit (pkgs) lib stdenv;
          cargoToml = builtins.fromTOML (builtins.readFile ./Cargo.toml);
          package = pkgs.rustPlatform.buildRustPackage {
            pname = "pocoshelf";
            version = cargoToml.package.version;

            src = lib.cleanSource ./.;
            cargoLock.lockFile = ./Cargo.lock;

            nativeBuildInputs = [
              pkgs.pkg-config
            ];

            buildInputs =
              [
                pkgs.libgit2
                pkgs.libssh2
                pkgs.openssl
                pkgs.zlib
              ]
              ++ lib.optionals stdenv.isLinux [
                pkgs.xorg.libX11
                pkgs.xorg.libxcb
              ]
              ++ lib.optionals stdenv.isDarwin (
                with pkgs.darwin.apple_sdk.frameworks;
                [
                  AppKit
                  CoreFoundation
                  CoreGraphics
                ]
              );

            env = {
              LIBSSH2_SYS_USE_PKG_CONFIG = "1";
              OPENSSL_NO_VENDOR = "1";
            };

            meta = {
              description = "Rust TUI file explorer with git-aware coloring";
              homepage = "https://github.com/rc-code-jp/pocoshelf";
              license = lib.licenses.mit;
              mainProgram = "pocoshelf";
              platforms = systems;
            };
          };
        in
        {
          default = package;
          pocoshelf = package;
        }
      );

      apps = forAllSystems (
        system:
        pkgs:
        let
          app = {
            type = "app";
            program = "${self.packages.${system}.default}/bin/pocoshelf";
          };
        in
        {
          default = app;
          pocoshelf = app;
        }
      );

      devShells = forAllSystems (system: pkgs: {
        default = pkgs.mkShell {
          inputsFrom = [
            self.packages.${system}.default
          ];
          packages = [
            pkgs.cargo
            pkgs.rustc
            pkgs.rustfmt
          ];
        };
      });
    };
}
