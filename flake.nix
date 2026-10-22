{
  description = "SimpleWarp is a local-only terminal with bring-your-own-key AI (Experimental Nix Support, Linux-only).";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    crane.url = "github:ipetkov/crane";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    {
      self,
      nixpkgs,
      crane,
      rust-overlay,
      ...
    }:
    let
      systems = [
        "x86_64-linux"
        "aarch64-linux"
      ];
      forAllSystems = nixpkgs.lib.genAttrs systems;
    in
    {
      packages = forAllSystems (
        system:
        let
          pkgs = import nixpkgs {
            inherit system;
            overlays = [ rust-overlay.overlays.default ];
          };
          lib = pkgs.lib;
          rustToolchain = pkgs.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml;
          rustPlatform = pkgs.makeRustPlatform {
            cargo = rustToolchain;
            rustc = rustToolchain;
          };
          craneLib = (crane.mkLib pkgs).overrideToolchain rustToolchain;
          appCargoToml = builtins.fromTOML (builtins.readFile ./app/Cargo.toml);
          version = "${appCargoToml.package.version}+${self.shortRev or "dirty"}";
          src = self;
          cargoLock = ./Cargo.lock;
          # Match Zed's approach: derive the main app vendor directory from
          # Cargo.lock instead of maintaining a top-level cargoVendorHash.
          cargoVendorDir =
            let
              craneVendorDir = craneLib.vendorCargoDeps {
                inherit src cargoLock;
                overrideVendorGitCheckout =
                  crates: drv:
                  let
                    hasCrate = crateName: builtins.any (crate: crate.name == crateName) crates;
                  in
                  drv.overrideAttrs (old: {
                    postPatch = (old.postPatch or "") + ''
                      find . -name 'Cargo.toml.orig' -delete

                      ${lib.optionalString (hasCrate "warp_multi_agent_api") ''
                        mkdir -p apis/multi_agent/v1/gen/rust/nix-vendored-protos
                        cp apis/multi_agent/v1/*.proto \
                          apis/multi_agent/v1/gen/rust/nix-vendored-protos/
                        substituteInPlace apis/multi_agent/v1/gen/rust/build.rs \
                          --replace-fail \
                            'let proto_path = manifest_dir.parent().unwrap().parent().unwrap();' \
                            'let proto_path = manifest_dir.join("nix-vendored-protos");'
                      ''}

                      ${lib.optionalString (hasCrate "warp-workflows") ''
                        mkdir -p workflows/nix-vendored-specs
                        cp -R specs/. workflows/nix-vendored-specs/
                        substituteInPlace workflows/build.rs \
                          --replace-fail \
                            'println!("cargo:rerun-if-changed=../specs");' \
                            'println!("cargo:rerun-if-changed=nix-vendored-specs");' \
                          --replace-fail \
                            'for entry in WalkDir::new("../specs") {' \
                            'for entry in WalkDir::new("nix-vendored-specs") {'
                      ''}
                    '';
                  });
              };
            in
            # crane writes a root config.toml; buildRustPackage expects the
            # cargoDeps layout to include .cargo/config.toml and Cargo.lock.
            pkgs.runCommand "simplewarp-${version}-cargo-vendor" { } ''
              cp -R ${craneVendorDir}/. "$out"
              chmod u+w "$out"
              mkdir -p "$out/.cargo"
              sed 's|${craneVendorDir}|@vendor@|g' \
                "$out/config.toml" > "$out/.cargo/config.toml"
              rm "$out/config.toml"
              cp ${cargoLock} "$out/Cargo.lock"
            '';

          linuxRuntimeLibraries = with pkgs; [
            alsa-lib
            curl
            dbus
            expat
            fontconfig
            freetype
            libGL
            libgit2
            libxkbcommon
            openssl
            stdenv.cc.cc.lib
            udev
            vulkan-loader
            wayland
            libx11
            libxscrnsaver
            libxcursor
            libxext
            libxfixes
            libxi
            libxrandr
            libxrender
            libxcb
            zlib
          ];

          simplewarp = rustPlatform.buildRustPackage {
            pname = "simplewarp";
            inherit version;

            inherit src;
            cargoDeps = cargoVendorDir;

            nativeBuildInputs = with pkgs; [
              brotli
              cargo-about
              clang
              cmake
              jq
              makeWrapper
              patchelf
              pkg-config
              protobuf
              python3
            ];

            buildInputs = linuxRuntimeLibraries;

            cargoBuildFlags = [
              "-p"
              "warp"
              "--bin"
              "simplewarp"
              "--bin"
              "generate_settings_schema"
            ];

            # The application test suite is large and GUI/integration-heavy; this
            # flake's package check is the Nix build plus a launch smoke test.
            doCheck = false;

            env = {
              LIBCLANG_PATH = "${pkgs.llvmPackages.libclang.lib}/lib";
              PROTOC = "${pkgs.protobuf}/bin/protoc";
              PROTOC_INCLUDE = "${pkgs.protobuf}/include";
              CARGO_PROFILE_RELEASE_DEBUG = "false";
            };
            postInstall =
              let
                installDir = "$out/opt/simplewarp";
                resourcesDir = "${installDir}/resources";
                libraryPath = lib.makeLibraryPath linuxRuntimeLibraries;
                executablePath = lib.makeBinPath (with pkgs; [ xdg-utils ]);
              in
              ''
                install -Dm755 "$out/bin/simplewarp" "${installDir}/simplewarp"
                rm -f "$out/bin/simplewarp"

                patchShebangs ./script/prepare_bundled_resources

                SKIP_SETTINGS_SCHEMA=1 ./script/prepare_bundled_resources \
                  "${resourcesDir}" \
                  release

                "$out/bin/generate_settings_schema" "${resourcesDir}/settings_schema.json"
                rm -f "$out/bin/generate_settings_schema"

                install -Dm644 \
                  "${resourcesDir}/THIRD_PARTY_LICENSES.txt" \
                  "$out/share/licenses/simplewarp/THIRD_PARTY_LICENSES.txt"

                install -Dm644 LICENSE-AGPL "$out/share/licenses/simplewarp/LICENSE-AGPL"
                install -Dm644 LICENSE-MIT "$out/share/licenses/simplewarp/LICENSE-MIT"

                install -Dm644 /dev/null "$out/share/applications/dev.simplewarp.SimpleWarp.desktop"
                cat > "$out/share/applications/dev.simplewarp.SimpleWarp.desktop" <<DESKTOP
                [Desktop Entry]
                Version=1.0

                Type=Application

                Name=SimpleWarp
                GenericName=TerminalEmulator

                Exec=simplewarp %U
                StartupWMClass=dev.simplewarp.SimpleWarp

                Keywords=shell;prompt;command;commandline;cmd;

                Icon=dev.simplewarp.SimpleWarp

                Categories=System;TerminalEmulator;

                # Don't run this application within a terminal.
                Terminal=false

                # Register ourselves as the handler for simplewarp:// URLs.
                MimeType=x-scheme-handler/simplewarp;
                DESKTOP

                for size in 16x16 32x32 64x64 128x128 256x256 512x512; do
                  icon="app/channels/stable/icon/no-padding/$size.png"
                  if [ -f "$icon" ]; then
                    install -Dm644 "$icon" \
                      "$out/share/icons/hicolor/$size/apps/dev.simplewarp.SimpleWarp.png"
                  fi
                done

                wrapProgram "${installDir}/simplewarp" \
                  --prefix LD_LIBRARY_PATH : "${libraryPath}" \
                  --prefix PATH : "${executablePath}"

                mkdir -p "$out/bin"
                ln -s "${installDir}/simplewarp" "$out/bin/simplewarp"
              '';

            postFixup = lib.optionalString pkgs.stdenv.isLinux ''
              wrapped="/opt/simplewarp/.simplewarp-wrapped"
              if [ -e "$out$wrapped" ] && ! patchelf --print-needed "$out$wrapped" | grep -q '^libfontconfig\.so\.1$'; then
                patchelf --add-needed libfontconfig.so.1 "$out$wrapped"
              fi
            '';

            meta = {
              description = "SimpleWarp is a local-only terminal with bring-your-own-key AI (Experimental Nix Support, Linux-only).";
              license = lib.licenses.agpl3Only;
              mainProgram = "simplewarp";
              platforms = systems;
              sourceProvenance = with lib.sourceTypes; [ fromSource ];
            };
          };
        in
        {
          inherit simplewarp;
        }
      );

      devShells = forAllSystems (
        system:
        let
          pkgs = import nixpkgs {
            inherit system;
            overlays = [ rust-overlay.overlays.default ];
          };
          lib = pkgs.lib;
          rustToolchain = pkgs.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml;
          nativeBuildInputs = with pkgs; [
            brotli
            cargo-about
            cargo-nextest
            clang
            cmake
            jq
            lld
            makeWrapper
            patchelf
            pkg-config
            protobuf
            python3
            rustToolchain
            rust-analyzer
          ];
          buildInputs = with pkgs; [
            alsa-lib
            curl
            dbus
            expat
            fontconfig
            freetype
            libGL
            libgit2
            libxkbcommon
            openssl
            stdenv.cc.cc.lib
            udev
            vulkan-loader
            wayland
            libx11
            libxscrnsaver
            libxcursor
            libxext
            libxfixes
            libxi
            libxrandr
            libxrender
            libxcb
            zlib
          ];
        in
        {
          default = pkgs.mkShell {
            inherit nativeBuildInputs buildInputs;
            LIBCLANG_PATH = "${pkgs.llvmPackages.libclang.lib}/lib";
            PROTOC = "${pkgs.protobuf}/bin/protoc";
            PROTOC_INCLUDE = "${pkgs.protobuf}/include";
            LD_LIBRARY_PATH = lib.makeLibraryPath buildInputs;
          };
        }
      );

      formatter = forAllSystems (system: nixpkgs.legacyPackages.${system}.nixfmt);
    };
}
