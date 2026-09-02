# Video Editor

[![Ask DeepWiki](https://deepwiki.com/badge.svg)](https://deepwiki.com/Liesegang/video-editor)

AviUtlの代替を目指した、Rustで書かれたオープンソースの動画編集ソフトウェアです。直感的なインターフェースと柔軟な拡張性を兼ね備え、高品質な動画編集を無料で提供することを目的としています（現在開発途中です）。

![プレビュー画面](https://github.com/user-attachments/assets/9c372278-cd8e-4c23-bc61-a581617bd042)

## 現在の編集モデル

RuViEは、配置と時間を扱う**Timeline**と、再利用可能な処理を扱う**Module**を分離しています。
通常の編集操作からNode Editorを開く必要はありません。

- **Timeline編集**：動画、音声、画像、Text、Solid、Nested Compositionを複数Trackへ配置できます。
- **直接操作**：移動、Trim、Split、Snap、Ripple Delete、親子付け、キーフレーム、FadeをTimelineとInspectorから編集できます。
- **Nested Composition**：Compositionは別種の編集モデルではなく、Timelineを入れ子にしたものです。
- **Effect Stack**：Node Moduleを使う処理も、通常画面ではEffectとして追加し、公開パラメータだけを編集します。
- **段階的な画面構成**：Beginner、Edit、Motion、Data、Logic、Diagnosticsの順に高度な機能を表示します。
- **PreviewとExport**：新しいTimeline ProjectをRenderPlanへコンパイルし、Preview、PNG、MP4を旧Projectへの変換なしで描画します。

編集データの所有権と評価順序は、[Timeline-first authoring model](docs/adr/0001-timeline-first-authoring.md)に記録しています。

## インストール（開発版）

現在開発中のため、安定版リリースはまだありません。開発版を試したい場合は以下のコマンドを実行してください。

```bash
git clone https://github.com/Liesegang/video-editor.git
cd video-editor
cargo run
```

### プラグインのビルドと読み込み

RuViE本体のビルド後でも、ABI v1に従うネイティブプラグインを追加できます。
サンプルとして、値を決定的に揺らす `random_property` evaluatorを用意しています。
これは本体のworkspaceには含まれず、`library` にリンクしません。

1. プラグインをビルドする

```bash
cargo build --manifest-path plugins/random_property/Cargo.toml --locked
```

2. 生成されたDLL/so/dylibを
   `plugins/random_property/ruvie-plugin.toml` と同じbundleディレクトリに置き、
   RuViEのruntime plugin pathへ配置する

本体を先にビルドし、pluginを別targetで後からビルド・配置して、変更前の
host binaryからdescriptor/default/evaluateまで確認するテストは次で実行できます。

```bash
./scripts/test-runtime-plugin.sh
```

ABI、bundle構成、対応categoryの詳細は
[Runtime native plugins](docs/runtime-plugins.md)を参照してください。

### PreviewとExport

ツールバーの `Export Frame` は、現在位置をPNGで保存します。
`Export Video` は、開いているTimelineをMP4（H.264、YUV 4:2:0）で保存します。
動画出力には、システムの `PATH` から実行できるFFmpegが必要です。

Projectファイルは、新しいTimeline-first schemaだけを読み書きします。
このリポジトリはpre-v1のため、旧形式のreader、migrator、双方向同期は提供しません。
切替前のコードはGit tag `pre-b-architecture-20260903` から復元できます。

## 開発への貢献

Video Editorの開発に参加したい方は、IssueやPull Requestを歓迎しています。

変更を送る前に、CIと同じRust品質ゲートを実行してください。

```bash
./scripts/quality-gate.sh
```

必要なネイティブ依存やlint方針、自己テストについては
[Rust quality gate](docs/rust-quality-gate.md)を参照してください。

- 改善や不具合報告は[Issueページ](https://github.com/Liesegang/video-editor/issues)へお願いします。
- コードの改善や新機能追加は、ForkしてPull Requestを作成してください。

## ライセンス

このプロジェクトは主に[MITライセンス](LICENSE)の下で公開されていますが、サードパーティコンポーネント（Qt、Skia、FFmpegなど）はそれぞれのプロジェクトのライセンスに従います。

詳細は[THIRD_PARTY_NOTICES.md](./THIRD_PARTY_NOTICES.md)を参照してください。

## 謝辞

サンプル動画は以下のクリエイター様の作品を使用させていただきました。

- **Blender Foundation**
  「Big Buck Bunny」
  ライセンス: CC BY 3.0
  https://peach.blender.org/
