use serde::{Deserialize, Serialize};

use super::{TimelineInterval, TimelineItemId, TranscriptDocumentId};

/// Imported transcript text and its stable source identity.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
#[serde(deny_unknown_fields)]
pub struct TranscriptDocument {
    pub id: TranscriptDocumentId,
    pub name: String,
    pub source: TranscriptSource,
    pub text: String,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TranscriptSource {
    SubRipFile { path: String },
    External { provider: String, source_id: String },
}

/// Lossless association between an editable subtitle item and source text/time.
/// Editing the displayed Text does not destroy this provenance.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
#[serde(deny_unknown_fields)]
pub struct TranscriptLink {
    pub item_id: TimelineItemId,
    pub document_id: TranscriptDocumentId,
    pub text_start: usize,
    pub text_end: usize,
    pub source_time: TimelineInterval,
}
