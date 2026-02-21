//! Migration utilities for project format upgrades.
//!
//! The previous migration (embedded effects/styles/ensembles → graph nodes)
//! is no longer needed because the embedded fields have been removed from TrackClip.
//! New projects always store effects, styles, effectors, and decorators as graph nodes.
