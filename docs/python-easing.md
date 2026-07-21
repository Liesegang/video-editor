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

The full quality gate sources the same environment and verifies the dependency
graph and exact interpreter version with `scripts/check-managed-cpython.sh`.
`RUVIE_CPYTHON_VERSION` may be overridden for an intentional runtime upgrade;
the runtime contract and package manifest must be updated in the same change.

Expressions run with Python builtins and imports. They are trusted code, not a
sandbox. Realtime audio never calls CPython per sample: until block/envelope
pre-evaluation is implemented, audio Expressions use their authored fallback
and report one diagnostic.

Packaged builds will place the pinned runtime and pure-Python `site-packages`
next to the application according to `python-runtime/cpython-runtime.json`.
That packaging step is separate from this source-build bootstrap.
