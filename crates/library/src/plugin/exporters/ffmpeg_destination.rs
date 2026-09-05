//! FFmpeg session reservation resolves the user-visible logical destination
//! with the same identity policy as the host-side export source-alias gate.
//! The separately supplied writable staging path never defines ownership.

pub(super) use crate::util::output_path_identity::{
    OutputPathIdentity as DestinationIdentity, output_path_identity as identity,
};
