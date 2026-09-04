# Rust quality gate

The repository uses one entry point locally and in CI:

```bash
./scripts/quality-gate.sh
```

It runs an explicit feature/target matrix. The default pass is required because
`--all-features` does not compile `cfg(not(feature = "..."))` branches; the
library CPU-fallback pass is required because both the default and maximal
workspace configurations enable `library/gl` from `crates/library`:

```bash
./scripts/quality-gate-self-test.sh
cargo fmt --all -- --check
cargo check --workspace --all-targets --locked
cargo clippy --workspace --lib --bins --locked -- -D warnings
cargo clippy -p library --lib --no-default-features --locked -- -D warnings
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --all-features --no-deps --locked
cargo test --workspace --all-targets --locked
cargo test --workspace --all-targets --all-features --locked
./scripts/dependency-audit.sh
```

The runtime `random_property` example is intentionally a standalone workspace,
so the gate separately runs rustfmt, check, the same Clippy policy, and tests
against `plugins/random_property/Cargo.toml` with its tracked lockfile. Removing
it from the host workspace therefore does not let maintained plugin code evade
the repository quality policy.

The Clippy commands also receive the opt-in lints listed in
`scripts/clippy-policy.sh`. CI installs the Linux development packages required
by FFmpeg, Skia, CPAL, PyO3, glutin, and winit before invoking the same script.
The macOS and Windows packaging workflows continue to validate their native
packaging paths separately.

Rust 1.95.0 is pinned in `rust-toolchain.toml`. Rustup users get that toolchain
automatically. A standalone Cargo installation, such as Homebrew Cargo, ignores
the rustup override file; it must provide Rust 1.95.0 plus Clippy and rustfmt.

On Windows, rust-skia publishes MSVC binaries but not Windows GNU binaries for
the pinned release. Bootstrap the repository once before invoking Cargo:

```powershell
cargo run -p xtask -- bootstrap
cargo build --release
```

The bootstrap installs and selects `1.95.0-x86_64-pc-windows-msvc`, installs
the pinned standard-GIL CPython 3.13.14 through `uv`, and writes a gitignored
machine-local Cargo configuration. The build copies that runtime beside
`app.exe`; direct execution does not depend on a system Python or environment
variables. A Windows GNU host is unsupported because it falls back to a large
Skia source build and does not match the packaged native dependencies.

The production/default Clippy pass intentionally selects only libraries and
binaries. Consequently the `allow-unwrap-in-tests`, `allow-expect-in-tests`, and
`allow-panic-in-tests` settings cannot weaken that production pass. The separate
all-target passes compile and test assertion-oriented test code under those
three narrowly scoped exceptions.

## Dependency audit

The shared entry point runs three exact, verified tool versions:

| tool | version | enforced check |
| --- | --- | --- |
| `cargo-audit` | 0.22.2 | RustSec vulnerabilities plus unmaintained, unsound, and yanked warnings |
| `cargo-deny` | 0.20.2 | allowed licenses, wildcard bans, and registry/Git source policy |
| `cargo-machete` | 0.9.2 | unused direct dependencies, using Cargo metadata to resolve renamed crates |

`scripts/dependency-audit.sh` installs a missing or mismatched version under
`target/quality-tools`; it never silently uses another version from `PATH`.
The CI workflow caches that directory. The initial audit removed unused direct
dependencies and optional AVIF support that the native image loader did not
advertise, then updated every advisory with a compatible fix. This reduced 14
vulnerability findings (12 distinct advisory IDs) and ten denied warning
findings to four reviewed exceptions.

Exceptions live in `quality/dependency-advisory-exceptions.txt`. Each record
must contain one exact advisory ID, a review deadline, an exact package and sole
direct parent, the human-readable locked path, runtime reachability, and a
remediation. The script validates every field and calendar date, rejects
duplicate IDs, verifies the current all-target inverse dependency tree, and
fails once any review date has passed before passing those exact IDs to
`cargo-audit`. A new runtime parent for an otherwise ignored package therefore
fails the path guard; wildcard or indefinite ignores are not supported.

| advisory | locked path | impact and reachability | required remediation | review by |
| --- | --- | --- | --- | --- |
| `RUSTSEC-2026-0194` | `quick-xml 0.39.4 <- wayland-scanner 0.31.10 <- winit/Wayland` | CPU denial of service in attribute parsing. Here `wayland-scanner` is a proc-macro and reads bundled protocol XML only during compilation; project and network XML do not reach it at runtime. | Move to `quick-xml >=0.41` when `wayland-scanner` permits it, normally through the egui/winit update. | 2026-10-31 |
| `RUSTSEC-2026-0195` | `quick-xml 0.39.4 <- wayland-scanner 0.31.10 <- winit/Wayland` | Memory denial of service in `NsReader`. The application does not use this parser at runtime; the locked path is build-time protocol generation. | Same upstream update as `RUSTSEC-2026-0194`. | 2026-10-31 |
| `RUSTSEC-2024-0436` | `paste 1.0.15 <- egui_dock 0.18.0 <- app` | Maintenance-status advisory, with no reported vulnerability. `paste` is a compile-time proc-macro; even current `egui_dock 0.20.1` still depends on it. | Remove when `egui_dock` drops `paste`, or migrate the dock implementation. | 2026-10-31 |
| `RUSTSEC-2026-0192` | `ttf-parser 0.25.1 <- owned_ttf_parser <- ab_glyph <- epaint 0.33.3 <- egui` | Maintenance-status advisory with no patched release in the egui 0.33 line. Font parsing is runtime-reachable, so it receives the shortest migration review rather than being classified unreachable. | Upgrade the egui ecosystem to 0.35 or later, which uses `skrifa`. | 2026-10-31 |

The accepted license list in `deny.toml` is the observed all-target lockfile
set: permissive licenses, font/data licenses, and the weak-copyleft MPL/LGPL
dependencies. The vendored parser's build-only CC0 dependency and Unicode name
data license are exact-package exceptions with recorded reasons. Unknown
registries, unknown Git sources, wildcard dependency requirements, unlicensed
workspace packages, and licenses outside that list fail the gate. Duplicate
transitive versions remain reported as warnings because the cross-platform
GUI/audio graph legitimately contains parallel platform API generations.

`rustpython-parser 0.4.0` is pinned as an audited source snapshot under
`third_party/rustpython-parser`. Its public parser and AST API remain unchanged;
the local patch only replaces the abandoned `unic-* 0.9.0` identifier and emoji
tables with `unicode-ident` and ICU4X. Provenance, the upstream checksum,
license, and the exact patch surface are recorded in the vendored directory's
`PATCHES.md`. This removes the unmaintained packages from `Cargo.lock` instead
of suppressing their RustSec advisories.

The 1,000-line ratchet applies to first-party Rust. Audited upstream snapshots
under `third_party/` retain their upstream/generated file layout and are
excluded explicitly; the executable policy fixture proves that an equally
large file anywhere in first-party source still fails.

## Executable policy test

`scripts/quality-gate-self-test.sh` applies the shared Clippy arguments to a
small independent crate. The valid fixture must pass, and each invalid feature
must fail with its expected lint. This catches accidental removal or misspelling
of the policy for unsafe pointer casts/transmutes, production-only cfg paths,
fallible `From`, process exit, path replacement, `dbg!`, `todo!`,
`unimplemented!`, ignored `Result`, redundant clone, oversized values/stack
arrays, undocumented unsafe blocks, and `unwrap()`, `expect()`, or `panic!()` in
production. Test targets may use the last three for assertion-oriented failures;
the valid fixture proves that this exception is limited to test code while the
invalid production fixtures fail.

The self-test also requires a tracked, non-ignored, current root `Cargo.lock`
and asserts the fail-closed command skeleton of `quality-gate.sh`. Removing fmt,
check, either feature-matrix Clippy/test pass, the self-test itself, or the
dependency audit therefore fails before the workspace build begins.
The full gate clears the dependency script's fixture-only validation and
exception-file overrides (and its tool-root override) before auditing, so a
stray test environment cannot turn the real audit into validation-only mode.

## Lint inventory and decisions

`scripts/lint-inventory.sh` uses compiler JSON rather than source-text matching
and evaluates every workspace target in the maximal `--all-features`
configuration. It is a promotion inventory, not a substitute for the default
and CPU-fallback gate passes above. The exhaustive nonzero `all`, `pedantic`,
`nursery`, and `restriction` snapshot, including a per-lint
adopt/defer/exclude decision, is recorded in
[`clippy-lint-inventory.md`](clippy-lint-inventory.md). `clippy::all` and the
curated safety/reliability lints are enforced now. Pedantic and nursery remain
measured promotion candidates; the restriction group is never enabled as a
blanket because it intentionally contains mutually exclusive policies.
