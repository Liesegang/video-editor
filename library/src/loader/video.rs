use crate::loader::image::Image;
use ffmpeg_next as ffmpeg;
use std::error::Error;

pub fn decode_video_frame(file_path: &str, frame_number: u64) -> Result<Image, Box<dyn Error>> {
    // ffmpeg を初期化
    ffmpeg::init()?;

    // 動画ファイルをオープン
    let mut ictx = ffmpeg::format::input(&file_path)?;

    // 最適な動画ストリームを取得
    let input = ictx
        .streams()
        .best(ffmpeg::media::Type::Video)
        .ok_or("動画ストリームが見つかりません")?;
    let video_stream_index = input.index();

    // ストリームのパラメータからデコーダコンテキストを作成
    let context_decoder = ffmpeg::codec::context::Context::from_parameters(input.parameters())?;
    let mut decoder = context_decoder.decoder().video()?;

    // 指定フレームを探すため、パケットを順次処理
    let mut decoded_frame = None;
    let mut current_frame: u64 = 0;
    for (stream, packet) in ictx.packets() {
        if stream.index() == video_stream_index {
            decoder.send_packet(&packet)?;
            let mut frame = ffmpeg::util::frame::Video::empty();
            // 受信可能なフレームをすべて受け取る
            while decoder.receive_frame(&mut frame).is_ok() {
                if current_frame == frame_number {
                    decoded_frame = Some(frame.clone());
                    break;
                }
                current_frame += 1;
            }
            if decoded_frame.is_some() {
                break;
            }
        }
    }
    // c
    decoder.send_eof()?;
    let mut frame = ffmpeg::util::frame::Video::empty();
    while decoder.receive_frame(&mut frame).is_ok() {
        if current_frame == frame_number {
            decoded_frame = Some(frame.clone());
            break;
        }
        current_frame += 1;
    }
    let frame = decoded_frame.ok_or("指定したフレームをデコードできませんでした")?;

    // スケーラーを用いてフレームを RGBA 形式に変換
    let mut scaler = ffmpeg::software::scaling::context::Context::get(
        decoder.format(),
        decoder.width(),
        decoder.height(),
        ffmpeg::format::Pixel::RGBA,
        decoder.width(),
        decoder.height(),
        ffmpeg::software::scaling::flag::Flags::BILINEAR,
    )?;
    let mut rgba_frame = ffmpeg::util::frame::Video::empty();
    scaler.run(&frame, &mut rgba_frame)?;

    // フレームから画像データをコピー（stride に対応）
    let width = rgba_frame.width();
    let height = rgba_frame.height();
    let row_bytes = (width * 4) as usize;
    let mut data = Vec::with_capacity(row_bytes * height as usize);
    let stride = rgba_frame.stride(0) as usize;
    let plane = rgba_frame.data(0);
    for y in 0..(height as usize) {
        let start = y * stride;
        let end = start + row_bytes;
        data.extend_from_slice(&plane[start..end]);
    }

    Ok(Image {
        width,
        height,
        data,
    })
}
