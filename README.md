# Video Editor

## Authoring architecture

RuViE is a timeline editor with explicitly authored, bounded Node Modules. A
normal video, audio, text, or nested-Timeline clip is never expanded into Nodes
in the Node Editor. A **Node Clip** is the deliberate Timeline placement of a
reusable Module; its placement and animation belong to the Timeline while its
processing connections belong to the Module Definition.

This keeps ordinary editing usable without the Node Editor and guarantees that
the number of normal Timeline items does not increase the number of Nodes. The
derived hierarchical Render Plan is runtime data and is not editable or stored
as the Project's source of truth.
Timeline-owned transitions use the same boundary: only their typed processing
is a bounded Module.

The two advanced editors have distinct names and responsibilities:

- **Curve Editor** edits keyframes, interpolation, and value-over-time curves.
- **Node Editor** edits processing Nodes and their connections inside one bounded Module.

### Reuse a clip

Right-click a Timeline clip and choose **Create Clip Prefab**. Its contents appear
under **Assets / Compositions**; drag that entry onto the Timeline to place linked
copies. Edit **Instance controls** in the Inspector to change one placement's
published values, or open the Composition to edit the shared contents. Direct Text
clips publish their text automatically.

Choose **Make Independent Copy** on a Composition clip to disconnect it from the
original shared contents. It remains an editable nested Timeline; any nested child
templates stay linked. Prefabs are currently reusable within the same Project.

### Reusable color ramps and GPU forces

In a Node Clip, search the Node Editor's context menu for **Gradient** and
**Color Ramp**. Connect a Gradient value to the ramp, then connect its Color
output to a color input such as Solid. The scalar Factor selects a color;
publish it to animate it with Timeline keyframes. Gradient values share the
existing Inspector/Node editor and Palette, including color stops and spread.

Particle Systems expose **Turbulence Strength** in the Inspector (zero by
default). Their Node graphs also support ordered, repeated Gravity, Drag,
Turbulence, Vortex, and Point forces, evaluated by the shared GPU compute
runtime.

### Sprite image collections

In a Particle System's Inspector, drag imported Image Assets onto **Sprites**
or select them from its thumbnail popup. Reorder or remove entries there;
removing an entry does not delete its Asset. Empty collections use the original
disc. **Tint** multiplies each image's color and alpha; new renderers start white.

**Selection Mode → random** gives each particle a stable image choice.
**value** uses **Selection** from 0 to 1 across the ordered collection (1 picks
the last image). Selection supports Timeline keyframes. For per-point choice,
unpublish Selection from its Node input context menu and connect Point Info's
Random, Normalized Age, or a stored Number attribute. An **Image Collection**
Data Node can supply Sprites after that input is explicitly unpublished.
Inspector and Nodes use the same collection editor and Project Asset previews.

Collections support up to 64 unique still Image Assets. Image aspect ratio and
transparency are preserved; collection edits do not restart the simulation.
Video, Timeline output, and Composition collections are not supported yet.

### Per-point custom attributes

Inside a Particle Node Clip, add **Point Info** and **Store Number Attribute**
from the context menu. Connect the final Force's Particles output to both
Points inputs; connect Store's Points output to Sprite Renderer's Points
input. Feed Normalized Age, Age, or Random from Point Info into Store's Value.
Store's Attribute output can then drive ordinary arithmetic Nodes and a
**Color Ramp**. The factory Sprite Tint is initially published to the Inspector:
right-click its Tint value and choose **Unpublish parameter** before connecting
the Color Ramp's Color output to it. This replaces the single Inspector color
control with per-point color logic explicitly.
Gradient values and frame-uniform arithmetic inputs retain their usual editors
and published Timeline keyframes.

Right-click a Store Node and edit **Name** to name the attribute (for example,
`heat`). Enter or clicking outside the field commits one undoable rename;
Escape cancels it. Empty or duplicate names within the same rendered Point
stream are rejected. Its stable identity does not depend on that name.
Stores can be chained, and later fields
read earlier attributes through their Attribute outputs. A field cannot read
another Point stream implicitly.

For procedural points without simulation, add **Point Grid** from the same
context menu and use its Points output in place of the Particle stream. Count
X/Y/Z, Spacing, Center, Size, and Seed are ordinary editable Node inputs. Grid
uses the same Store, arithmetic, Color Ramp, and Sprite renderer. Use Point
Info's **Random** output with Grid; Age and Normalized Age require particles.
Lattice point identities survive axis-count changes, so surviving points keep
their random values. Each axis supports up to 1,024 points, with 100,000 total.

Store Attribute Nodes support Number, Integer, Boolean, Vec2, Vec3, Vec4, and Color.
For example, connect a Color Ramp to **Store Color Attribute** and use its
Attribute output as Sprite Color. **Point Info → Position** supplies a Vec3
field in producer-local coordinates, before the Sprite transform; capture it
with **Store Vec3 Attribute**. Typed Attribute outputs can feed later Stores
without changing their type. Integer attributes preserve exact signed 32-bit
values, and Color attributes use working-linear color through the GPU path.
The ordinary Add, Subtract, Multiply, Divide, and Fmod Nodes work on Number
and Vec2/3/4 fields. A scalar broadcasts to every vector component; two
vectors must have the same dimension. **Length** returns the magnitude of a
vector (or the absolute value of a scalar), both for ordinary values and
per-point fields. For example, Position → Multiply → Store Vec3 Attribute →
Length → Divide → Color Ramp produces distance-based Point colors. Capturing
or calculating a position attribute does not move the source points.
To move points, insert **Set Point Position** into the Points stream. Its
Position input accepts a Vec3 field, including a stored attribute; left
unconnected and unpublished, it uses the incoming position. Offset adds a
Vec3 displacement, and the Boolean Selection input limits which points move.
Offset starts at zero, so the default operation leaves the points in place.
Point Info connected before the operation reads the original position;
connected after it, it reads the changed position. Other branches and the
Particle simulation remain unchanged. Use the existing header bypass control
to pass the incoming Points through, and publish Offset or Selection to animate
them with Timeline keyframes.
Frame-uniform inputs retain their shared editors, published parameters, and
Timeline keyframes. Integer field arithmetic and implicit per-point type
conversions are not supported.

The **Logic** menu provides Less Than, Less Than or Equal, Greater Than, Greater
Than or Equal, Equal, and Not Equal comparisons of scalar numbers. Their result is a
Boolean, not a numeric 0/1. Capture it with **Store Boolean Attribute** to reuse
a per-point mask. **Select Number/Integer/Boolean/Vec2/Vec3/Vec4/Color** chooses
between two values of the indicated type using its Boolean Condition input.
For example, `heat` → Greater Than → Store Boolean Attribute → Select Color
colors points differently above and below a threshold. Uniform inputs retain
the same shared editors and published Timeline keyframes.
Select evaluates both inputs; it is value selection, not short-circuit control
flow. An invalid unselected input still invalidates the result, so Select is
not a guard for zero division. Comparisons use exact finite-number relations,
with no implicit epsilon, and varying Integer-to-Number conversion is rejected.

Attributes are computed on the GPU at each rendered frame (after simulation
for particles). They are not yet accumulated across simulation steps.
Programs are bounded to 16 attributes, 64 instructions, and 8 Color Ramps with
64 stops each. Invalid per-point arithmetic produces a transparent Sprite for
that point. Runtime arrays and GPU programs are not saved into Projects.

## Repository layout

All Cargo packages owned by the host application live under `crates/` and are
members of the root workspace. Package names remain stable, so development and
CI commands should select them with `cargo ... -p <package>` from the repository
root instead of depending on a package's filesystem path.

`plugins/<plugin-id>/` is reserved for independently distributed plugin
bundles. Each plugin owns its manifest and lockfile and is intentionally not a
root-workspace member; this keeps the stable plugin boundary honest and proves
that a plugin does not link to host-internal crates. `examples/` contains
standalone third-party integration examples for the same reason. Neither tree
is a duplicate location for host packages.

### Windows development build

Run the Rust bootstrap task once. After that, ordinary Cargo builds and tests
reuse the existing managed runtime; they do not install Python again:

```powershell
cargo run -p xtask -- bootstrap
cargo build --release
.\target\release\app.exe
```

Create the reusable Windows distribution directory with the Rust publish task:

```powershell
cargo xtask publish
```

The executable does not use a system Python installation, and publishing does
not produce an archive implicitly. The ready-to-run directory is written to
`target/publish/windows-x86_64/RuViE`.

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

動画出力はアプリのExportダイアログから実行します。FFmpegバイナリはシステムPATH上にある前提です（必要に応じて`ffmpeg_path`プロパティで明示できます）。

## 開発への貢献

Video Editorの開発に参加したい方は、IssueやPull Requestを歓迎しています。

変更を送る前に、CIと同じRust品質ゲートを実行してください。

```bash
./scripts/quality-gate.sh
```

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
