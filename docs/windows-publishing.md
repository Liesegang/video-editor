# Windows publishing

RuViE uses a Rust `xtask` for Windows setup and publishing. No PowerShell
script is part of the repository or release process.

## One-time setup

Install `rustup` and `uv`, then run:

```powershell
cargo run -p xtask -- bootstrap
```

The task selects the pinned MSVC Rust toolchain, finds an existing uv-managed
standard-GIL CPython 3.13.14, and downloads it only when it is absent. It writes
a gitignored `.cargo/config.toml` so later Cargo commands use that exact
interpreter.

Routine `cargo build` and `cargo test` commands never install Python.

## Publish

```powershell
cargo xtask publish
```

The command builds the release application and creates the directly runnable
directory at `target/publish/windows-x86_64/RuViE`. It does not create a ZIP or
installer. CI artifact storage may archive the directory as a transport detail.

Publishing stages and validates the complete directory before replacing an
older publication. It includes:

- `app.exe` and its application-local native dependencies;
- the pinned private CPython runtime and standard library;
- FFmpeg and the DLL dependency closure matching the built executable;
- runtime assets and license notices;
- `bundle-manifest.json` with byte lengths and SHA-256 hashes.

Set `RUVIE_FFMPEG_DIR` when FFmpeg is not discoverable through the pinned
vcpkg installation or `PATH`:

```powershell
$env:RUVIE_FFMPEG_DIR = 'C:\path\to\ffmpeg'
cargo xtask publish
```

For an already-built release, or a custom publication location:

```powershell
cargo xtask publish --skip-build
cargo xtask publish --output 'D:\Releases\RuViE'
```

`--skip-build` is intended for local packaging/QA iteration only. Normal
releases should let the publish task perform its locked release build.
