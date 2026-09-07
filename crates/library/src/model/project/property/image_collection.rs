use std::collections::HashSet;
use std::fmt;

use serde::de::Error as _;
use serde::ser::{Error as _, SerializeStruct};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use uuid::Uuid;

const IMAGE_COLLECTION_VALUE_TAG: &str = "image_collection_value";
pub const IMAGE_COLLECTION_MAX_ASSETS: usize = 64;

#[derive(Clone, PartialEq, Eq, Debug, Hash, Default)]
pub struct ImageCollectionValue {
    pub assets: Vec<Uuid>,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum ImageCollectionValueError {
    TooManyAssets { count: usize },
    DuplicateAsset { asset_id: Uuid },
}

impl ImageCollectionValue {
    pub fn new(assets: Vec<Uuid>) -> Result<Self, ImageCollectionValueError> {
        let value = Self { assets };
        value.validate()?;
        Ok(value)
    }

    pub fn validate(&self) -> Result<(), ImageCollectionValueError> {
        if self.assets.len() > IMAGE_COLLECTION_MAX_ASSETS {
            return Err(ImageCollectionValueError::TooManyAssets {
                count: self.assets.len(),
            });
        }
        let mut unique = HashSet::new();
        for asset_id in &self.assets {
            if !unique.insert(*asset_id) {
                return Err(ImageCollectionValueError::DuplicateAsset {
                    asset_id: *asset_id,
                });
            }
        }
        Ok(())
    }
}

impl fmt::Display for ImageCollectionValueError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooManyAssets { count } => write!(
                formatter,
                "Image Collection has {count} assets; at most {IMAGE_COLLECTION_MAX_ASSETS} are allowed"
            ),
            Self::DuplicateAsset { asset_id } => {
                write!(formatter, "Image Collection repeats Asset {asset_id}")
            }
        }
    }
}

impl std::error::Error for ImageCollectionValueError {}

impl Serialize for ImageCollectionValue {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        self.validate().map_err(S::Error::custom)?;
        let mut state = serializer.serialize_struct("ImageCollectionValue", 2)?;
        state.serialize_field("$type", IMAGE_COLLECTION_VALUE_TAG)?;
        state.serialize_field("assets", &self.assets)?;
        state.end()
    }
}

impl<'de> Deserialize<'de> for ImageCollectionValue {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Wire {
            #[serde(rename = "$type")]
            value_type: String,
            assets: Vec<Uuid>,
        }

        let wire = Wire::deserialize(deserializer)?;
        if wire.value_type != IMAGE_COLLECTION_VALUE_TAG {
            return Err(D::Error::custom(format!(
                "Image Collection value tag must be {IMAGE_COLLECTION_VALUE_TAG:?}, got {:?}",
                wire.value_type
            )));
        }
        Self::new(wire.assets).map_err(D::Error::custom)
    }
}

pub(crate) fn has_image_collection_value_tag_json(value: &serde_json::Value) -> bool {
    value
        .as_object()
        .and_then(|object| object.get("$type"))
        .and_then(serde_json::Value::as_str)
        == Some(IMAGE_COLLECTION_VALUE_TAG)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strict_tag_order_and_bounds_round_trip() {
        let first = Uuid::new_v4();
        let second = Uuid::new_v4();
        let value = ImageCollectionValue::new(vec![first, second]).unwrap();
        let encoded = serde_json::to_value(&value).unwrap();
        assert_eq!(encoded["$type"], "image_collection_value");
        assert_eq!(
            serde_json::from_value::<ImageCollectionValue>(encoded).unwrap(),
            value
        );
        assert!(
            serde_json::from_value::<ImageCollectionValue>(serde_json::json!({
                "assets": []
            }))
            .is_err()
        );
        assert!(ImageCollectionValue::new(vec![first, first]).is_err());
        assert!(
            ImageCollectionValue::new(vec![Uuid::new_v4(); IMAGE_COLLECTION_MAX_ASSETS + 1])
                .is_err()
        );
    }

    #[test]
    fn property_value_keeps_collection_tag_strict_and_visits_nested_references() {
        use crate::model::property::PropertyValue;

        let asset_id = Uuid::new_v4();
        let value = PropertyValue::Array(vec![PropertyValue::ImageCollection(
            ImageCollectionValue::new(vec![asset_id]).unwrap(),
        )]);
        let restored: PropertyValue =
            serde_json::from_value(serde_json::to_value(&value).unwrap()).unwrap();
        assert_eq!(restored, value);
        let mut visited = Vec::new();
        restored.visit_image_asset_ids(&mut |id| visited.push(id));
        assert_eq!(visited, vec![asset_id]);

        let missing_tag: PropertyValue = serde_json::from_value(serde_json::json!({
            "assets": [asset_id]
        }))
        .unwrap();
        assert!(!matches!(missing_tag, PropertyValue::ImageCollection(_)));
    }

    #[test]
    fn collection_interpolation_is_stepwise_and_keeps_order() {
        use crate::model::property::PropertyValue;

        let first = PropertyValue::ImageCollection(
            ImageCollectionValue::new(vec![Uuid::new_v4(), Uuid::new_v4()]).unwrap(),
        );
        let second = PropertyValue::ImageCollection(
            ImageCollectionValue::new(vec![Uuid::new_v4()]).unwrap(),
        );
        assert_eq!(PropertyValue::interpolate(&first, &second, 0.75), first);
        assert_eq!(PropertyValue::interpolate(&first, &second, 1.0), second);
    }
}
