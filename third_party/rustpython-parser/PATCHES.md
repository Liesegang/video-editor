# Vendored rustpython-parser

- Upstream crate: `rustpython-parser 0.4.0`
- Registry checksum: `868f724daac0caf9bd36d38caf45819905193a901e8f1c983345a68e18fb2abb`
- Upstream repository revision: `8dd2aea26778d8d6917770f8e32bea1b9cdc0ae8`
- Source: <https://crates.io/crates/rustpython-parser/0.4.0>
- Vendored: 2026-07-20
- License: MIT; see `LICENSE`

The source snapshot is the crates.io 0.4.0 package. RuViE carries one focused
maintenance patch because that release directly depends on the abandoned
`unic-* 0.9.0` family:

1. `Cargo.toml` and its upstream `Cargo.toml.orig` replace `unic-ucd-ident`
   with maintained `unicode-ident` and `unic-emoji-char` with maintained
   `icu_properties`.
   The root workspace also lists this snapshot under `workspace.exclude` so
   tooling keeps third-party source separate from first-party members.
   The manifest records cargo-machete's exact build-script and
   feature-forwarding false positives; the dependencies remain exercised by
   the vendored build and workspace feature matrix.
2. `src/lexer.rs` keeps the same two predicates and parser API, sourcing XID
   classification from `unicode-ident` and `Emoji_Presentation` from ICU4X.

No parser grammar, generated parser, AST contract, or public API is changed.
The workspace Expression tests are the compatibility suite for this patch.
The root `.gitattributes` preserves unrelated upstream whitespace byte-for-byte.

To refresh the snapshot, unpack the newer crates.io package, reapply only the
Unicode dependency patch if upstream still needs it, update the provenance
above, and run `./scripts/quality-gate.sh`. Do not add a RustSec ignore for the
old `unic-*` packages.
