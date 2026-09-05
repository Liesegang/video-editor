# ADR 0008: Shape Appearance、Paint、Color Palette

- Status: Accepted
- Date: 2026-09-05
- Updated: 2026-09-06（共通 alpha mask、描画範囲、Text の組版共有）
- Owners: Core Model / Renderer / Motion UX（担当者未指定）
- Depends on: ADR 0001、ADR 0007

## Context

通常の Shape Clip は、現行 authoring model では geometry parameter と単一 Fill 相当の色しか持たない。

一方、production 資産には `DrawStyle`、Fill/Stroke descriptor、複数 Style branch と Merge、semantic Style stack、Effect Stack の drag UI がある。

これらは再利用できるが、旧Project graphを通常Shapeへ再接続すると、単純な見た目までユーザー向けNodeとして展開される。

また、Gradientを固定個数の色parameterやEffectとして追加すると、Color picker、Palette、Canvas handle、animation、再利用swatchの間で別々の値を持つことになる。

Shapeの見た目はTimelineから直接編集でき、必要な場合だけ有限のNode Moduleへ昇格できなければならない。

## Decision

### 所有権

Shapeのgeometryとappearanceを分離する。

通常の Shape Clip では、`ShapeSource` がgeometryを、Timeline-owned `AppearanceStack` が直接編集された見た目とautomationを所有する。

Text Clipにも同じ所有権を適用し、文字内容とlayoutからAppearanceを分離する。

Projectは再利用可能な `PaintDefinition` とPalette順序を所有する。

Node Moduleは、明示的にNode Clipへ昇格したAppearance、または再利用ロジックの内部だけを所有する。

RenderPlanはAppearanceを描画passへcompileするが、source of truthにはならない。

同じAppearanceをTimelineとNode graphへ同時に所有させない。

### PaintはColorの別名ではない

Paintを次のtagged unionとして扱う。

```rust
enum Paint {
    Solid(ColorValue),
    Gradient(GradientPaint),
    Pattern(PatternPaint),
}
```

FillとStrokeのpaint fieldは `PaintBinding` を受け取る。

単色だけに意味があるparameterは `ColorValue` のまま残す。

Gradientを受け取れないparameterへ暗黙に平均色や先頭stopを渡さない。

```rust
enum PaintBinding {
    Inline(Paint),
    Linked(PaintDefinitionId),
}
```

通常編集は `Inline` を変更し、そのinstanceだけへ反映する。

`Linked` を編集する操作は影響instance数を事前表示し、Project serviceの一transactionでDefinitionを更新する。

Project内の参照中Definitionはそのまま削除できない。

削除を確定した場合は、全参照を現在のresolved Paintによる `Inline` へ同じtransactionでdetachしてからDefinitionを削除する。

このため、missing Definition用の別のlive値を持たずに見た目を保持できる。

### 第一級Gradient

Gradientはstable identityを持つ値である。

```rust
struct GradientPaint {
    id: GradientId,
    kind: GradientKind,
    stops: Vec<GradientStop>,
    spread: GradientSpread,
    interpolation: GradientInterpolation,
    geometry: GradientGeometry,
}

struct GradientStop {
    id: GradientStopId,
    position: AutomatableParameter,
    color: AutomatableParameter,
    opacity: AutomatableParameter,
    midpoint: AutomatableParameter,
}
```

`GradientKind` は少なくともLinear、Radial、Conic、Freeformを区別する。

種類固有のcenter、focal point、radius、angle、mesh pointは `GradientGeometry` に置き、曖昧な汎用number配列へ入れない。

StopのIDは追加、削除、並べ替え後も変えない。

Curve EditorとBindingは配列indexではなくstable stop IDと公開parameterを参照する。

Stop colorは元のmanaged `ColorValue` を保持する。

補間時に各stopをProject working-linear spaceへ変換し、Preview/Export terminalでのみdisplay/output変換する。

表示用sRGBへ変換した8-bit値をauthoring値へ自動で書き戻さない。

### Appearance stack

Appearanceは順序付きの有限treeである。

```rust
struct AppearanceStack {
    entries: Vec<AppearanceEntry>,
}

struct AppearanceEntry {
    id: AppearanceEntryId,
    name: String,
    enabled: bool,
    opacity: AutomatableParameter,
    blend_mode: BlendMode,
    local_transform: AutomatableTransform,
    target: AppearanceTarget,
    operation: AppearanceOperation,
}

enum AppearanceOperation {
    Fill(FillAppearance),
    Stroke(StrokeAppearance),
    Effect(AppearanceProcessorInvocation),
    Group(AppearanceStack),
}

enum AppearanceTarget {
    WholeShape,
    Group(ShapeGroupId),
    Subpath(ShapeSubpathId),
}
```

Entry、Group、SubpathのIDはProject保存対象とする。

Path editorはgeometryを書き換えても、維持できるSubpath IDを再採番しない。

対象が消えたAppearanceは別のsubpathへ黙って付け替えない。

状態を `Active`、`Orphaned`、`Conflict` として保持し、Inspectorから解決する。

FillとStrokeは任意数を許可する。

Strokeはwidth、alignment、join、cap、miter、dash、offset、arrowhead、width profile、brushを型付きfieldとして持つ。

Effectは適用対象をEntry/Group/WholeShapeのtree位置で表し、名前文字列や現在の配列indexでは参照しない。

### Project Palette

Project fileは次を保存する。

```rust
struct ProjectPalette {
    definitions: HashMap<PaintDefinitionId, PaintDefinition>,
    groups: Vec<PaletteGroup>,
    ungrouped_order: Vec<PaintDefinitionId>,
}

struct PaintDefinition {
    id: PaintDefinitionId,
    name: String,
    paint: Paint,
    tags: Vec<String>,
}
```

Mapと表示順を分離し、drag reorderでDefinition IDや参照を変更しない。

Group間移動、rename、duplicate、delete、Paint編集はProject serviceのcommandとして実装する。

各commandはvalidation、dirty state、Undo/Redo、局所invalidationを一回だけ発生させる。

組込みPaletteとユーザーlibraryはProjectのsource of truthではない。

選択したswatchはProjectへcopyまたは明示的importしてから使い、別PCでも同じProjectが再現できるようにする。

外部Palette formatのimporter/exporterはplugin capabilityとして追加できる。

### UI

Paint対応propertyは、既存の共通Color swatchと同じ位置から一つのPaint pickerを開く。

Popup内で `Picker` と `Palette` を切り替える。

別の一時Palette windowや上部toolbar buttonを主要導線にしない。

Solid編集では現行のlossless Color pickerとcolor-management変換をそのまま使う。

Palette viewはSolid、Gradient、Patternを同じgridへ描き、検索、group、rename、duplicate、delete、drag reorderを提供する。

Paint typeを切り替えた場合だけAppearance fieldの型を変更する。

Gradient選択時はstop editorを表示し、Canvas上に方向、中心、焦点、半径handleを出す。

InspectorとCanvasは同じGradient/stop IDへcommandを送る。

Appearance panelは既存Effect Stackのproperty row、property-mode icon、drag payload、insertion preview、context menuを共通primitiveへ抽出して使う。

上移動、下移動、削除を `...` に隠した代替UIは作らない。

### RenderPlanとrenderer

CompilerはAppearance treeを階層的なShape style/effect/composite passへ変換する。

同じlinked PaintDefinitionはcompiled paint resourceを共有する。

Instance parameterだけを変更した場合に、無関係なgeometryやModule Definitionを再compileしない。

Cache dependencyにはgeometry revision、Appearance entry revision、PaintDefinition revision、relevant animation range、plugin processor versionを含める。

GPU rendererはGradient/Patternをretained resourceとして扱い、frameごとのCPU readback/reuploadを行わない。

CPUとGPUは同じworking-linear補間、premultiplication、blend順を満たす。

### 本体alphaとレイヤースタイルの合成順

Drop Shadowの元になるalphaは、FillとStrokeを合成した本体から作る。
生のgeometryを一律に塗ったmaskでは、Strokeだけの図形の空洞が埋まり、Fillのoffsetや透明度も反映されない。
Shape、Text、Ensemble Textは、同じrenderer内の本体描画とmask処理を使う。

その上で、各レイヤースタイルを次の順に合成する。
同じ段階に属するstyle同士は、ユーザーが指定した順序を維持する。

| 段階 | 描画対象 |
| --- | --- |
| 背面 | Drop Shadow、Outer Glow |
| 本体 | Fill、Stroke |
| 前面 | Color/Gradient/Pattern Overlay、Inner Shadow/Glow、Satin、Bevel/Emboss |

Ensembleでは、変形後の全文字を本体として扱ってから影を合成する。
文字ごとに影と本体を交互に描くと、後続文字の影が先行文字を覆うためである。
Fill/Strokeを含まないstyle専用描画ではgeometryのsilhouetteを使うが、明示的に透明なFillを置いた場合はその透明alphaを維持する。

本体の描画記録とmaskのsource filterは、同じレイヤー内のstyleへ共有する。
記録はlocal座標を保ち、影の距離やblurの大きさにも本体と同じ変形を適用する。
描画範囲を制限するboundsと、Gradientの座標を決めるboundsは分ける。
Fill/Strokeだけのレイヤーには、mask用の描画記録を追加しない。

複数のPath partを持つShapeも、一つの本体として合成する。
各partの透明度は、そのpartのFill/Stroke全体へ一回だけ適用し、合成した本体から影を生成する。
Styleの透明度を書き換えたり、partごとのFrameObjectへ分割したりすると、影の透明度の二重適用や描画順の逆転を起こす。
このため、評価済みのpartを既存のShape描画要求へ保持し、レイヤーのEffectも合成後に一回だけ適用する。
これは実行時の描画データであり、二つ目の編集モデルではない。

### Textの組版とEnsemble

通常TextとEnsemble Textは、一つのSkParagraphから得た字形を同じ描画処理で使う。
文字列全体を組版した後、Paragraphの描画用TextBlobから実際のFont、glyph ID、位置、行原点、UTF-8の対応範囲を取得する。
Ensembleはこの字形の変形と色を変更し、切り出した文字列を別のFontで組版し直さない。
このため、何も変化させないTransformや完了後のStep Delayだけで字詰めや代替フォントが変わる二経路を持たない。

EnsembleのChar単位は、Unicodeの書記素を字形のcluster境界に合わせた**不可分な文字要素**とする。
一つの合字が複数の書記素にまたがる場合は、その全範囲を一要素として扱う。
結合文字や絵文字の途中を独立して移動したり、合字の後半に指定された変更を黙って無視したりしない。
要素は論理的なUTF-8/UTF-16範囲と行内順序を持ち、描画は元の視覚的なrun順序を維持する。
Tracking、Step Delay、手動patchはこの同じ要素を対象にする。

本体描画はStyleの順序を外側に、連続する字形のrunを内側にして行う。
同じ変形と色を持つ隣接字形はまとめて描くが、Fontや字形の位置は変更しない。
通常Textだけを別の描画関数へ逃がすidentity判定は設けない。
本体alphaと影も、この共通描画を使う。
字形の描画と外形計算は既存のTransformから得た同じaffineを使用する。

Fontとglyphを含む組版結果はrender-localな派生値とする。
Projectに二つ目の文字モデルを保存せず、使われていないEnsemble専用のChar/Line/Textモデルは削除する。
検証には通常Textとneutral Ensembleの比較に加え、独立してParagraphを直接描くテストを残す。
二つの新経路が同じ誤りを持っていても、相互比較だけでは検出できないためである。

### Inspectorと時間編集の所有権

通常TextのEnsembleとText/ShapeのAppearanceは、既存のoperation内のPropertyを編集する。
TimelineとCurve Editorは、そのPropertyのowner、key、keyframe IDを共通のautomation laneから参照する。
operationごとの別の曲線モデルは持たず、キーの移動もInspectorと同じ編集serviceへ渡す。
同名のPropertyが複数のoperationにあっても、operation IDで区別する。

明示的なNode Clip化後は、公開parameterの定数をModule Instanceが、キーをTimeline上のinvocationが所有する。
変換時はキーのID、ローカル時刻、値、補間を保持し、曲線の参照先を公開parameterへ切り替える。
通常TextのoperationとModule内部を双方向同期しない。

Curve Editorは、ユーザーが明示的に隠したチャンネルを一時UI状態として保持する。
新しくキーを作ったPropertyや、Node Clip化で参照先が公開parameterへ変わったチャンネルは既定で表示する。
現在表示中のID集合だけを保持すると、同じClipへの編集で生じた新しいチャンネルが非表示のまま残るためである。

キーのドラッグは、eguiが記録した押下位置からの総移動量を共通CanvasTransformで時刻と値へ変換する。
押下位置は一時的なドラッグ状態に保持し、領域外への移動やreleaseと同時に届く最後の移動も同じ処理で評価する。
フレームごとの移動量を開始時の値へ加えると、入力頻度によって結果が変わるため使用しない。
ベクトルのキーは全成分で一つの時刻を共有し、縦方向の移動は選んだ成分だけへ反映する。
曲線とキーの表示もこの投影値を使い、確定時に既存の編集serviceへ一回だけ渡す。

Curveのドラッグ中も、Inspectorと共通のProperty投影から映像Previewを描画する。
移動対象は型付きownerと既存のkeyframe IDで指定し、投影と確定が同じ検証済み更新処理を使う。
時刻を指定してキーを追加し直すと元のキーが残るため、移動にはupsertを使わない。
同じキーの時刻と値を更新し、補間と未操作の成分は保持する。
Curve側のドラッグ状態から投影を導出し、別の一時編集状態へ複製しない。
開始後にProjectのrevisionが変わった場合は投影と確定を取り消し、新しい編集を古いドラッグ値で上書きしない。
Escでは投影を破棄し、キャンセル前に送信した描画結果も確定済みPreviewへ混入させない。

縦軸の表示範囲はCurve Editorのview状態として保持する。
キーを編集した直後に範囲を自動計算し直すと、確定した点がカーソルから離れるためである。
対象Clipの切替と明示的なFitで表示範囲を求め直し、まだキーがないチャンネルからは範囲を固定しない。
この範囲はProjectへ保存する曲線データではない。

Inspectorの数値ドラッグは、一つの一時編集状態からPreviewへ投影する。
通常Propertyと公開parameterでは値の保存先が異なるため、投影先を型で区別し、各保存先の既存編集処理を使う。
ドラッグ中はProjectのrevisionとUndo履歴を変更せず、release時に一回だけ確定する。
選択、revision、再生位置が変わった場合は既存のInspector同期処理で投影を破棄する。
公開parameterの投影では、別Instanceへの変更やTimeline automationを定数で置き換える要求を拒否する。

投影したProjectには、その投影から導出したRenderPlanを組み合わせる。
公開parameterの値とautomationはCompiledModuleInvocationにも保持されるため、確定前のRenderPlanではドラッグ中の値を描画できない。
同じincremental compiler cacheで投影をコンパイルし、変更のないModule本体とTimeline scheduleを共有する。
revisionと一時編集のdigestが同じ間は投影用planを再利用し、pan/zoomだけでは再コンパイルしない。
投影の失敗時は送信待ちの要求を破棄し、確定済みProjectとplanのcacheは変更しない。

### 外形と描画範囲

グラデーションの座標と、ぼかし処理に必要な余白は別の用途を持つ。
同じ矩形を使うと、影の大きさやCompositionの解像度を変えただけで本体のグラデーションが変化する。
そこで、既存rendererで解決したgeometryから次の三つの範囲を求める。

| 範囲 | 用途 |
| --- | --- |
| geometry | 装飾前のPathまたは文字のink。Gradient Overlayの0→1の座標基準。 |
| content | Fill/Stroke、Path effect、輪郭のAAを含む本体。maskの描画記録の範囲。 |
| visual | contentに影や光彩の余白を加えた範囲。filterの描画とstyle合成用saveLayerの範囲。 |

通常Textのinkは、描画に使うSkParagraphのglyph boundsから求める。
Ensembleでは、文字ごとの本体を変形してから範囲を合併し、その外側へレイヤースタイルの余白を加える。
Strokeのmiterとcap、およびBevelのblur kernelも範囲の計算へ反映する。
合成段階と余白の計算はmodelとrendererで共有し、独立した定数を追加しない。

直接合成するText、Shape、SkSLの一時Surfaceは、変形後のvisual範囲を現在の描画先へ切り詰めて確保する。
端は整数pixelへ外向きに丸め、AAとfilterの丸めに備えてdevice座標で2 pixelの余白を取る。
描画時はSurfaceの原点移動を相殺し、local座標のgeometry、mask、styleを従来と同じ処理へ渡す。
最終合成では整数原点へ無補間で描き戻し、Dissolveの粒を決める座標も元の描画先を基準にする。
Ensembleの既存Backplateもvisual範囲に含めるが、本体alphaやGradientのgeometryへ混ぜない。
複数Pathを持つShapeのBackplateは、本体と同じpartの外形を使い、表示用のaggregate fallbackから別の位置を算出しない。

外部Image Effectへ渡すraster境界は、引き続き描画先全体の画像を返す。
この境界には部分画像の原点を表す契約がないためである。
直接合成とraster境界は、同じSurface生成と本体描画を使い、物理的な確保範囲だけを変える。
最終出力とCompositionのgroup Surfaceは全体サイズを維持する。

4Kの小さなTextとShapeについて、1 layerと16 layersのwarm Previewをproduction benchmarkで比較する。
この計測にはGPU描画、terminal color変換、RGBA8 readbackを含むが、事前のframe評価とUIへのuploadは含まない。
実際の割当byte数を取得するcounterは未実装であり、面積の計算値を実測値として記録しない。
製品全体の60 fpsは、別の実測gateで判定する。

### 明示的なNode Clip昇格

`Convert Shape Appearance to Node Clip` は、現在のAppearanceを有限graphへ一度だけ変換する。

変換結果はShape branch、Style operation、一つのAppearance Stack node、必要なEffect operation、一つのOutput terminalから成る。
Styleが一つの場合も同じ構造を使う。

この合成には、画像から区別できる**Style値**を導入する。
Style値はdescriptorで評価した描画設定であり、Shapeの画像でも、別途永続化する編集モデルでもない。

| 出力またはnode | 入力と意味 |
| --- | --- |
| Style operationの`style`出力 | parameterからStyle値を返す。Shape入力は使わない。 |
| Style operationの`image`出力 | Shape入力へ単独のstyleを適用して画像を返す。 |
| Appearance Stack node | 一つのShape入力と順序付きのStyle入力を受け、共通rendererで本体と装飾を合成する。 |

通常のImageはStyle入力へ接続できない。
単独Styleの画像出力は、そのstyleだけで描画する用途として維持する。
しかし複数styleを個別のImageへ変換してから汎用Image Mergeへ渡すと、本体alphaを共有できず、影や半透明Fillの画素が変わる。
そのため、汎用Image Mergeの意味を変えたり、変換元のmetadataから合成方法を切り替えたりして等価性を補わない。

通常ShapeやTimeline全体をNodeへ展開しない。

変換成功後はModule graphだけが内部Appearanceを所有し、元のAppearanceとの双方向同期を行わない。

一回Undoは変換前のShapeSourceとAppearanceStackを完全に復元する。

旧 `SemanticStyleStack` はadapterとして接続しない。

その順序、stable identity、property wire、blend保持のアルゴリズムとテスト仕様だけを現行authoring serviceへ移植する。

### Plugin boundary

Appearance processor descriptorは、入力と出力のShape/Style/Image型、parameter contract、CPU/GPU capability、color boundary、determinismを宣言する。

PluginはAppearanceStackやProjectを直接変更しない。

追加、編集、削除はvalidated proposalをCore serviceがtransactionとして適用する。

Frame、Gradient sample、Path pointを一要素ずつJSONでhot pathへ渡さない。

## Vertical slices

### Slice A: Palette foundation

ProjectPalette、Solid PaintDefinition、validation、service command、save/load、Undo/Redoを実装する。

既存Color pickerのPopupへPalette tabを追加し、Solid swatchの追加、適用、rename、drag reorder、削除をnative HTTP QAする。

### Slice B: Multiple Fill/Stroke

通常ShapeへAppearanceStackを追加し、複数Solid Fill/Stroke、entry enable/opacity/blend/orderをproduction rendererへ通す。

Inspector dragとimage goldenを追加する。

### Slice C: Linear/Radial Gradient

Gradient/stop model、working-linear補間、Paint picker、Palette保存、Canvas handle、automationを実装する。

広色域、HDR、alpha、stop reorderをgolden/native QAする。

### Slice D: TargetとGroup

Stable Group/Subpath ID、nested Appearance Group、orphan/conflict reconciliationを実装する。

Path編集後のtarget保持をテストする。

### Slice E: Advanced PaintとEffect

Conic/Freeform Gradient、Pattern、brush/profile、Appearance Effectをtyped descriptorとして追加する。

### Slice F: Node promotion

Appearanceからbounded Module graphへの等価変換、golden、one Undo、共有Definition semanticsを実装する。

## Validation and acceptance

- ShapeにFillを三つ、Strokeを二つ置いても通常UIのユーザー向けNode数は増えない。
- Entry drag中に最終composite順のpreviewを表示し、release一回で一つのUndo entryを作る。
- 一つのinstanceのFill色を変えてもsibling instanceは変わらない。
- Linked swatch編集時だけ全参照が変わり、事前に影響数を表示する。
- Project保存/loadでDefinition、Palette順、Gradient stop ID、Appearance targetが一致する。
- Color pickerを開閉しただけではmanaged f64値を変更しない。
- GradientのCPU/GPU PreviewとExportが許容差内で一致する。
- Node Clip昇格前後のpixels、entry順、blend、animationが一致する。
- Stroke-onlyとDrop Shadow、offset FillとDrop Shadow、半透明FillとDrop Shadowの組合せでも、Node Clip昇格前後の画素が一致する。
- GPUの輪郭AA差はstyleを付けない基準描画から独立に判定する。輪郭以外の画素、透明な空洞、shadow alphaの検証を緩めるために使わない。
- PaletteとAppearanceの基本操作をproduction native HTTP QAで実座標から操作する。

## Rejected alternatives

### 通常ShapeのFill/Strokeを自動でNode化する

単純な装飾ほどNode数が増え、今回解消する問題を再導入するため採用しない。

### GradientをEffectとして実装する

Fill/Strokeのpaint identity、Palette、Canvas geometry、stop automationを分断するため採用しない。

### PaletteをUI preferenceだけへ保存する

別環境でProjectの見た目と参照が再現できないため採用しない。

### ColorValueへGradient用fieldを追加する

単色とPaintの型契約が曖昧になり、単色consumerに暗黙変換が必要になるため採用しない。

### 旧graph modelへ薄いadapterを追加する

二つのownerと長期同期を生むため採用しない。
