use crate::plugin::{DecodedPixelBuffer, DecodedPixelDescription};
use lru::LruCache;
use std::hash::Hash;
use std::num::NonZeroUsize;

const DEFAULT_ENTRY_CAPACITY: usize = 128;
const DEFAULT_BYTE_BUDGET: usize = 256 * 1024 * 1024;

/// One decoded video result. Pixel bytes and their post-decode color contract
/// intentionally share this value so cache hits cannot reconstruct metadata
/// from mutable decoder state.
#[derive(Clone, Debug)]
pub(super) struct DecodedVideoFrame {
    pub(super) pixels: DecodedPixelBuffer,
    pub(super) decoded: DecodedPixelDescription,
}

/// Count- and byte-bounded LRU for decoded video frames.
///
/// Count alone is unsafe for large media: 128 RGBA8 4K frames consume roughly
/// four GiB. Frames larger than the whole budget bypass this cache.
pub(super) struct DecodedFrameCache<K> {
    entries: LruCache<K, DecodedVideoFrame>,
    resident_bytes: usize,
    byte_budget: usize,
}

impl<K: Hash + Eq> Default for DecodedFrameCache<K> {
    fn default() -> Self {
        Self::new(
            NonZeroUsize::new(DEFAULT_ENTRY_CAPACITY).unwrap_or(NonZeroUsize::MIN),
            DEFAULT_BYTE_BUDGET,
        )
    }
}

impl<K: Hash + Eq> DecodedFrameCache<K> {
    fn new(entry_capacity: NonZeroUsize, byte_budget: usize) -> Self {
        Self {
            entries: LruCache::new(entry_capacity),
            resident_bytes: 0,
            byte_budget,
        }
    }

    pub(super) fn get(&mut self, key: &K) -> Option<DecodedVideoFrame> {
        self.entries.get(key).cloned()
    }

    pub(super) fn put(&mut self, key: K, frame: DecodedVideoFrame) {
        let frame_bytes = frame.pixels.byte_len();
        if frame_bytes > self.byte_budget {
            return;
        }
        if let Some(replaced) = self.entries.pop(&key) {
            self.resident_bytes = self
                .resident_bytes
                .saturating_sub(replaced.pixels.byte_len());
        }
        while self.resident_bytes.saturating_add(frame_bytes) > self.byte_budget {
            let Some((_key, evicted)) = self.entries.pop_lru() else {
                break;
            };
            self.resident_bytes = self
                .resident_bytes
                .saturating_sub(evicted.pixels.byte_len());
        }
        self.resident_bytes = self.resident_bytes.saturating_add(frame_bytes);
        if let Some((_key, evicted)) = self.entries.push(key, frame) {
            self.resident_bytes = self
                .resident_bytes
                .saturating_sub(evicted.pixels.byte_len());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{DecodedFrameCache, DecodedVideoFrame};
    use crate::model::frame::Image;
    use crate::plugin::{DecodedPixelBuffer, DecodedPixelDescription, DecodedStraightRgba32F};
    use std::num::NonZeroUsize;

    #[test]
    fn pixels_and_descriptor_remain_together_under_the_byte_budget() {
        let capacity = NonZeroUsize::new(4).unwrap_or(NonZeroUsize::MIN);
        let mut cache = DecodedFrameCache::new(capacity, 8);
        let expected = DecodedPixelDescription::unverified("synthetic descriptor");
        cache.put(
            7_i64,
            DecodedVideoFrame {
                pixels: rgba8(vec![1, 2, 3, 255]),
                decoded: expected.clone(),
            },
        );

        let Some(hit) = cache.get(&7) else {
            panic!("one four-byte frame should fit the cache budget");
        };
        assert_eq!(
            hit.pixels.as_rgba8().map(|image| image.data.as_slice()),
            Some([1, 2, 3, 255].as_slice())
        );
        assert_eq!(hit.decoded, expected);
        assert_eq!(cache.resident_bytes, 4);

        for key in [8_i64, 9_i64] {
            cache.put(
                key,
                DecodedVideoFrame {
                    pixels: rgba8(vec![0, 0, 0, 255]),
                    decoded: DecodedPixelDescription::abi_v1_srgb(),
                },
            );
        }
        assert!(cache.get(&7).is_none(), "byte budget must evict LRU");
        assert!(cache.get(&9).is_some());
        assert_eq!(cache.resident_bytes, 8);

        cache.put(
            10_i64,
            DecodedVideoFrame {
                pixels: rgba8(vec![0; 12]),
                decoded: DecodedPixelDescription::abi_v1_srgb(),
            },
        );
        assert!(cache.get(&10).is_none());
        assert_eq!(cache.resident_bytes, 8);
    }

    #[test]
    fn float_payloads_are_charged_at_their_actual_resident_size() {
        let capacity = NonZeroUsize::new(4).unwrap_or(NonZeroUsize::MIN);
        let mut cache = DecodedFrameCache::new(capacity, 31);
        let pixels = DecodedStraightRgba32F::new(2, 1, vec![[0.0, 0.0, 0.0, 1.0]; 2])
            .expect("valid float fixture");
        cache.put(
            1_i64,
            DecodedVideoFrame {
                pixels: DecodedPixelBuffer::StraightRgba32F(pixels),
                decoded: DecodedPixelDescription::abi_v1_srgb(),
            },
        );

        assert!(
            cache.get(&1).is_none(),
            "32-byte frame must bypass 31-byte cache"
        );
        assert_eq!(cache.resident_bytes, 0);
    }

    fn rgba8(data: Vec<u8>) -> DecodedPixelBuffer {
        crate::plugin::LoadResponse::rgba8(
            Image::new((data.len() / 4) as u32, 1, data),
            DecodedPixelDescription::abi_v1_srgb(),
        )
        .expect("valid RGBA8 fixture")
        .into_parts()
        .0
    }
}
