# Video Editor

[![Ask DeepWiki](https://deepwiki.com/badge.svg)](https://deepwiki.com/Liesegang/video-editor)

AviUtlの代替を目指した、Rustで書かれたオープンソースの動画編集ソフトウェアです。直感的なインターフェースと柔軟な拡張性を兼ね備え、高品質な動画編集を無料で提供することを目的としています（現在開発途中です）。

![プレビュー画面](https://github.com/user-attachments/assets/9c372278-cd8e-4c23-bc61-a581617bd042)

## 特徴（予定）

- **直感的なUI**: 初心者から上級者まで幅広く利用可能な使いやすいインターフェース
- **マルチトラック編集**: 動画、音声、画像を無制限のトラックで編集可能
- **豊富なエフェクトとフィルター**: プラグインで自由に拡張可能なエフェクトやフィルター機能
- **クロスプラットフォーム対応**: Windows、macOS、Linuxで動作
- **完全オープンソース**: MITライセンスに基づいて公開され、自由な改変と再配布が可能

## インストール（開発版）

現在開発中のため、安定版リリースはまだありません。開発版を試したい場合は以下のコマンドを実行してください。

```bash
git clone https://github.com/Liesegang/video-editor.git
cd video-editor
cargo run
```

### プラグインのビルドと読み込み

プロパティ用プラグインを DLL/so として後からロードできます。サンプルとしてランダムに値を揺らす `random_noise` プロパティプラグインを用意しています。

1. プラグインをビルドする

```bash
cargo build -p random_property_plugin
```

Windows の場合は `target\debug\random_property_plugin.dll`、Linux の場合は `target/debug/librandom_property_plugin.so` が生成されます。

2. `library` バイナリ起動時に DLL を渡してロードする

```bash
cargo run -p library -- test_data/project.json target/debug/random_property_plugin.dll
```

`test_data/project.json` では `random_noise` プロパティを使用しており、プラグインを読み込むと回転に揺らぎが加わります。

### FFmpeg エクスポーター

`export` ブロックをプロジェクト JSON に追加すると、動画を書き出すフォーマットをプロパティで指定できます。例えば:

```json
"export": {
  "container": { "type": "constant", "properties": { "value": "mp4" } },
  "codec": { "type": "constant", "properties": { "value": "libx264" } },
  "pixel_format": { "type": "constant", "properties": { "value": "yuv420p" } },
  "bitrate": { "type": "constant", "properties": { "value": 8000.0 } },
  "quality": { "type": "constant", "properties": { "value": 23.0 } }
}
```

- `container`: 出力コンテナ (`mp4`, `mkv` など)。`png` を指定すると従来通り連番画像を書き出します。
- `codec`: FFmpeg のコーデック名 (`libx264`, `libx265` など)。
- `pixel_format`: 出力ピクセルフォーマット (`yuv420p`, `rgba` 等)。
- `bitrate`: kbps 単位の映像ビットレート (任意)。
- `quality`: H.264 の CRF など品質値 (任意)。

設定を `mp4` などにした場合、`cargo run -p library -- ...` を実行すると `./rendered/<composition>.mp4` が生成されます。FFmpeg バイナリはシステム PATH 上にある前提です (必要に応じて `ffmpeg_path` プロパティで明示できます)。

## 開発への貢献

Video Editorの開発に参加したい方は、IssueやPull Requestを歓迎しています。

- 改善や不具合報告は[Issueページ](https://github.com/Liesegang/video-editor/issues)へお願いします。
- コードの改善や新機能追加は、ForkしてPull Requestを作成してください。

## ライセンス

このプロジェクトは主に[MITライセンス](LICENSE)の下で公開されていますが、サードパーティコンポーネント（Qt、Skia、FFmpegなど）はそれぞれのプロジェクトのライセンスに従います。

詳細は[THIRD_PARTY_NOTICES.md](./THIRD_PARTY_NOTICES.md)を参照してください。

## 謝辞

サンプル画像、動画は以下のクリエイター様の作品を使用させていただきました。

- **SDx3 様**
  「カートゥーンな海と空」
  ライセンス: CC0
  https://www.pixiv.net/artworks/83263525

- **Blender Foundation**
  「Big Buck Bunny」
  ライセンス: CC BY 3.0
  https://peach.blender.org/

- **test_sound.mp3**
  「作業中のテーマ -Working now-」
  ライセンス: CC BY 3.0
  https://soundcloud.com/user-80857924/working-now
