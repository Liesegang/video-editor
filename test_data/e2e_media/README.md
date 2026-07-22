# Deterministic media fixtures

These tiny files exercise the real image and FFmpeg loaders without relying on
the large manual test assets in the parent directory.

Run `./generate.sh` from any directory to regenerate the fixtures and
`SHA256SUMS`. The script requires an FFmpeg build with libx264, libvpx-vp9,
FFV1, and libmp3lame encoders, plus `ffprobe` and the official `cwebp` tool.
`manifest.json` records the loader contract and the semantic role of every
fixture; `SHA256SUMS` records the checked-in bytes.

`av_duration_mismatch.mp4` intentionally has a 12-frame/1-second video stream
and a 2-second audio stream. It covers both embedded-audio hydration and the
rule that `Asset.frame_count`, not padded container duration, bounds video.

`multi_audio.mkv` has a video at global stream 0, a left-only 440 Hz signal at
global stream 1, and a right-only 880 Hz signal at global stream 2. Tests use
these global FFmpeg ordinals explicitly and verify that the decoded signals do
not collapse to the first audio track.

`vfr_pts.mkv` advertises 10 fps but has display timestamps at 0.0, 0.1, 0.5,
and 1.8 seconds. The generator verifies those exact PTS with `ffprobe`; the
Rust tests assert frame selection by those timestamps instead of by an average
FPS-derived ordinal.

`color_rec709_limited.mp4`, `color_rec2020_pq.mp4`, and
`color_rec2020_hlg.mp4` carry explicit H.273 source tags. The matching
`color_untagged.mp4` deliberately omits them; tests must keep those fields
unknown rather than guessing a color space from dimensions or codec.

Verify the checked-in bytes from the fixture directory:

```sh
(cd test_data/e2e_media && shasum -a 256 -c SHA256SUMS)
```

Encoded-file hashes can vary after intentionally changing the FFmpeg version.
The Rust tests additionally hash decoded RGBA output and compare frames by
semantics, so a fixture regeneration must be reviewed rather than silently
accepted.
