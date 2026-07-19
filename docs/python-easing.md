# Python expression easing

The default editor build does not link a host `libpython`. This keeps normal
core, test, and GUI builds independent of Xcode's Python framework layout and
prevents failures such as a stale `Python3.framework/Versions/3.9/lib` search
path followed by `library 'python3.9' not found`.

`EasingFunction::Expression` remains part of the Project model and continues
to execute by default. The default mode keeps a `python3` worker process and
communicates over JSON lines, so no Python library is linked into the editor.
Set `VIDEO_EDITOR_PYTHON` when the desired interpreter is not `python3` or
`python`:

```sh
VIDEO_EDITOR_PYTHON=/usr/local/bin/python3 cargo run -p app
```

An embedded PyO3 runtime is also available as an explicit build feature:

```sh
cargo run -p app --features python-easing
```

PyO3's embedding mode needs a Python installation that supplies a linkable
library. If discovery selects an Xcode command-line-tools Python whose library
directory no longer exists, select a complete installation before building:

```sh
PYO3_PYTHON=/opt/homebrew/bin/python3 \
  cargo run -p app --features python-easing
```

The repository check below verifies that the default normal dependency graph
does not contain PyO3:

```sh
./scripts/check-default-no-libpython.sh
```
