//! FFmpeg's session reservation uses the same path identity as the host-side
//! export source-alias gate.

pub(super) use crate::util::output_path_identity::{
    OutputPathIdentity as DestinationIdentity, output_path_identity as identity,
};
