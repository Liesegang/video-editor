# Embedded CPython expressions

RuViE uses a trusted, in-process CPython 3.13 runtime for Property Expressions,
Python easing, future automation, and Python-authored plugins. It does not use
the removed RustPython parser or a RuViE-owned Python IR/evaluator.

The source checkout pins CPython 3.13.14 through `uv`. Install it once:

```sh
./scripts/bootstrap-managed-python.sh
```

Compile, test, and launch through the wrapper so PyO3's build-time interpreter
and RuViE's runtime home always refer to that same distribution:

```sh
./scripts/with-managed-python.sh cargo run -p app
./scripts/with-managed-python.sh cargo test --workspace
```

On Windows, the repository bootstrap performs the equivalent setup and writes
a gitignored, machine-local Cargo configuration. A normal release build then
copies the pinned runtime beside the executable, so the executable can be
launched directly without Python, uv, PATH changes, or environment variables:

```powershell
cargo run -p xtask -- bootstrap
cargo build --release
.\target\release\app.exe
```

Bootstrap installs the pinned interpreter only when it is absent. Routine
builds, tests, and `cargo xtask publish` reuse it without reinstalling.

The full quality gate sources the same environment and verifies the dependency
graph and exact interpreter version with `scripts/check-managed-cpython.sh`.
`RUVIE_CPYTHON_VERSION` may be overridden for an intentional runtime upgrade;
the runtime contract and package manifest must be updated in the same change.

Expressions run with Python builtins and imports. They are trusted code, not a
sandbox. Realtime audio never calls CPython per sample: until block/envelope
pre-evaluation is implemented, audio Expressions use their authored fallback
and report one diagnostic.

Windows source and published builds place the pinned runtime next to the
application according to `crates/python-runtime/cpython-runtime.json`. Published
applications always prefer that private runtime over a developer environment
override. The publisher never copies the build machine's `site-packages`;
shipping third-party Python packages requires an explicitly locked input.
