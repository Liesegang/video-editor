# RuViE 実装バックログ

この文書は RuViE の今後の実装作業を管理する唯一のバックログです。
新しい機能案、調査結果、受入条件はここへ統合し、別の並行ロードマップを作りません。
上から依存関係を潰し、コード、テスト、native HTTP QA、エラーログ確認まで完了して初めてチェックを付けます。
作業 goal は、依存が解決済みの未完了チェック項目から一つの vertical slice を選んで設定し、完了した同じ commit でこの文書の状態も更新します。

## ステータスの読み方

- `[x]` **完了**：現在のリポジトリに実装と相応の自動テストまたは native QA の根拠がある。
- `[ ]` **部分実装**：根拠となるコードはあるが、同じ項目に列挙した不足または最終 QA が残っている。
- `[ ]` **未実装**：宣言、ADR、カタログ項目だけの場合を含み、利用可能な機能としては扱わない。
- 完了状態は 2026-09-05 時点のコードと監査結果に基づく。
  存在する型名やメニュー名だけで完了とは判定しない。

## 変更不能の設計原則

- Timeline は時間配置、Track、Clip、レイヤー順、親子関係、Nested Timeline、直接編集、Keyframe、Mask、Matte、Transition、Marker、Tempo を所有する。
- Node Editor は、明示的な Node Clip または Module Attachment の内側にある有限の処理グラフだけを編集する。通常 Clip や Timeline 全体を Node へ展開しない。
- 外部接続は Published Parameter、Signal、Event、Action、Media Port のみを参照し、Module 内部 Node UUID を参照しない。
- RenderPlan は階層を維持した派生データであり、Project の source of truth にせず、ユーザーにも編集させない。
- プラグインは機能を追加できるが、Timeline の所有権、Project 検証、Undo/Redo、永続化、実行スケジューリングを迂回できない。
- 初心者向け操作に Node、Port、Binding、RenderPlan という語を出さない。高度化によって基本操作の手数を増やさない。
- pre-v1 のため、廃止モデル向け reader、writer、migrator、双方向同期、互換 evaluator を追加しない。
- 既存 production 実装を実際の責務境界で拡張する。コピー、別名実装、薄い adapter による二重化を禁止する。
- UI、model、service、runtime、renderer、persistence、test、tooling の全層で DRY を守り、各 source/test/QA ファイルは 1,000 行未満に保つ。

## 依存順

```text
M0 Baseline / repository hygiene
  -> M1 Authoring ownership / RenderPlan / bounded Node Clip
     -> M2 Production editor restoration
        -> M3 Cut editing / Transition
        -> M4 Audio / Musical timeline / DTM
     -> M5 Extensible plugin kernel
        -> M6 Workflow and provider plugins
     -> M7 Unified 3D / Particle / FBX
M3 + M4 + M5 + M6 + M7
  -> M8 Realtime performance / export / publish
     -> M9 Persona acceptance
```

M4、M6、M7 は M1 と各契約が固まった後に並行してよいが、M2 の基本編集を壊した状態で先へ進めません。

---

## M0: Baseline、復旧点、リポジトリ構造

- [x] **完了：復旧点を固定する。** `pre-production-ui-reset-20260904` と `recovery/before-production-ui-reset-20260904` が復元点を指す。以後、比較のために production UI の挙動と見た目を参照し、旧 evaluator を本番へ戻さない。

- [x] **完了：Rust workspace package を `crates/` へ集約する。** host が所有する `app`、`library`、`color-management`、`plugin-api`、`python-runtime`、`pan-zoom-ui`、`node-editor-ui`、`xtask` は `crates/<name>` に配置され、root `Cargo.toml` もその配置を参照している。次を満たして完了とする。
  - 旧 root package directory が Git 差分上も削除され、同じ source の二重配置がない。
  - `plugins/<plugin-id>` は独立配布する plugin bundle、`examples/` は外部実装例として workspace package 集約の対象外であることを明文化する。
  - portable script、docs、CI、fixture の manifest path を全て新配置へ直す。
  - clean checkout で `cargo metadata --locked`、workspace build/test、publish が通る。

- [ ] **部分実装：production baseline を記録する。** 復旧 tag と現 main について、project load、first frame、seek、edit-to-preview、連続再生、audio、export、100/1,000/10,000 Clip、同一 Module 多数配置、GPU/CPU/メモリを同じ fixture で計測する。数値、OS、GPU、driver、release profile、fixture hash を `docs/performance/` に保存し、未計測の「速い」を完了条件に使わない。

- [ ] **部分実装：コード品質の継続ゲートを固定する。** `AGENTS.md` の再利用、共通 surface、DRY、1,000 行制限を CI で検査し、`rg` ベースの境界検査だけでなく Rust dependency graph と source line count も検証する。
  - [x] `scripts/check-source-file-size.sh` を CI の fail-closed quality gate に組み込み、first-party の Rust/Python/shell/JS/TS/C/C++/SkSL を 1,000 行以下に固定した。tracked と non-ignored untracked の両方を NUL-safe に検査する。
  - [ ] 重複 owner、恒久的な `new`/`legacy`/`timeline_first`/意味のない `v2`、名称責務、`.ps1` 混入は、機械検査できる範囲を追加し終えるまでレビュー規約だけで完了扱いにしない。
  - 同じ責務の重複 owner がない。
  - `new`、`legacy`、`timeline_first`、意味のない `v2` を恒久 module 名にしない。
  - repository automation に `.ps1` を commit しない。
  - `Node Editor` と `Curve Editor` の名称と責務を混同しない。

- [ ] **未実装：小さく commit/push する運用を固定する。** 一つの検証可能な vertical slice ごとに commit し、対象 unit test、`cargo check`、`git diff --check` が green になった時点で main へ push する。UI slice は対象 native HTTP QA も green にしてから push し、長時間の未 push 差分を作らない。milestone 完了時は復旧 tag を追加する。

## M1: Timeline ownership、階層 RenderPlan、限定 Node Clip

- [x] **完了：B 案の所有権と Node island 契約を固定する。** `docs/adr/0001-node-islands.md` に Timeline、Module Definition/Instance/Invocation、Published Interface、InstancePath、RenderPlan の責務と禁止事項がある。

- [x] **完了：authoring model の基本 vertical slice を実装する。** `TimelineItem`、`SourceRef`、Nested Timeline、`ModuleDefinition`、`ModuleInstance`、`ModuleInvocation`、`InstancePath` と階層 RenderPlan compiler が存在し、compiled definition は invocation ごとに展開せず共有される。

- [ ] **部分実装：廃止した graph-owned authoring model を閉じる。** production authoring path から Track/Clip/Composition の `node_ids`、structural merge、CompositionInstance-as-Node を参照できなくし、境界検査を通す。旧 model/evaluator を互換経路として残さず、必要な低レベル graph primitive だけを Module 内部へ移す。

- [ ] **部分実装：RenderPlan の incremental invalidation を完成する。** Definition executable hash、asset fingerprint、instance parameter、relevant Timeline range、binding dependency、generator version を cache key/dependency に持たせる。
  - 字幕一文字または Clip 一つの移動で無関係な Project 全体を再 compile/render しない。
  - Definition 不変で instance 値だけが変わる場合、Module executable を再 compile しない。
  - 同じ Lower Third 100 個は `CompiledModuleDefinition: 1`、`ModuleInvocation: 100` になる。

- [ ] **部分実装：通常 Clip を明示的に Node Clip へ昇格する。** `convert_source_to_node_clip` と native QA は存在するが、embedded Audio を持つ Video は現在拒否される。完了条件は次の通り。
  - Image、Video+Audio、Audio、Text、Shape、Nested Timeline の対応可能な source を、右クリックの明示操作で一つの bounded Node Clip に変換できる。
  - source、Effect、Ensemble の意味と pixels/audio/timing を失わず、移動対象と外側に残す Attachment を transaction preview で説明する。
  - 一回の Undo で元へ戻り、sibling/shared Definition は変わらない。
  - Timeline keyframe を複数の Published Parameter/Media/Signal 入力として公開し、Timeline/Dope Sheet/Curve Editor が時間を所有し続ける。
  - 通常 Clip 数に比例して Node 数が増えず、変換前後の golden image/audio が一致する。

- [ ] **部分実装：Module host を Node Clip 以外へ一般化する。** Item/Track/Composition/Bus/Master/Project Control の Attachment host を同じ `ModuleInvocation` 契約で追加する。他レイヤーや Nested Timeline を displacement/matte 等の入力にするときは Published Media Input と `InstancePath` binding を使い、対象 Timeline を Node 化しない。

- [ ] **部分実装：Signal/Event binding runtime を完成する。** `docs/adr/0002-event-runtime.md` は public contract のみ完了している。
  - Signal は base、keyframe、各 binding、manual override の寄与と operator/smoothing を deterministic に合成する。
  - Inspector は effective-value provenance を表示し、Canvas で直接操作しても binding を黙って切らない。
  - Event は `Restart`、`IgnoreWhilePlaying`、`Queue`、`Overlap` を実装し、`Overlap` は永続 Item の clock 書換えではなく bounded reactive instance を生成する。
  - 同じ Definition の複数 Nested placement の片方だけを `InstancePath` で制御できる。

- [ ] **未実装：GeneratedItem/Override を第一級にする。** generator/source row の stable key、generator version、provenance と manual patch を保存し、再生成で同じ item の修正を維持する。消滅/競合時は `Active`、`Orphaned`、`Conflict` を黙って捨てず、解決 UI と Undo を提供する。

## M2: production editor surface の復旧と統合

- [ ] **部分実装：共通 viewport を唯一の navigation 実装にする。** Timeline、Curve Editor、Node Editor、Preview は `pan-zoom-ui::CanvasState` と application `ViewportController` を使用しているが、全 native interaction の回帰確認が残る。
  - grid、content、hit test、selection、gizmo、overlay、QA metadata が同じ transform から導出され、pan/zoom 中に相対位置がずれない。
  - Node zoom が freeze せず、Timeline の通常 drag が意図せず scroll に化けない。
  - Timeline/Curve の playhead は canvas clip rect 内だけに描画し、Curve の channel list や表示時間外へ出ない。
  - Curve の Y range は少なくとも絶対値 100,000 を fit/zoom でき、finite/zero-range を安全に処理する。

- [ ] **部分実装：expandable Track/Clip Timeline を production UX として完成する。** 一つの Track に複数 Clip を置き、Track を layer のように並べ、展開時に Clip と keyframed property lane を表示する。
  - Assets から Timeline panel への drag/drop が主要導線で、`Add to timeline` や `Drag sources to Timeline` の仮ボタン/説明を置かない。
  - drag 中の Clip は pointer 下の安定した project time に留まり、preview が振動しない。
  - 下の Clip を動かしても、明示的な linked selection/ripple scope でない上の Clipは動かない。
  - Track/layer 並べ替え中に insertion gap と全 row の入替 preview を表示する。
  - vertical zoom で Track/Clip height を変え、thumbnail、waveform、node topology、keyframe の表示モードを選べる。
  - property lane は「変更済み」ではなく keyframe/automation がある property だけを表示する。

- [ ] **部分実装：Assets panel を完成する。** row overlap をなくし、Table/List と preview Icon/Grid を切り替えられる。名前、種類、size、duration、fps、sample rate/channels 等は panel 幅に応じて省略表示/tooltip/column resize で読め、選択 asset の image/video/audio/shape preview と metadata を Inspector に表示する。

- [ ] **部分実装：Preview の直接編集を production parity へ戻す。** 共通 viewport 上で grid/pan/zoom、object click selection、空白 click deselection、正しい geometry bounds の gizmo、move/scale/rotate を扱う。
  - 円、Shape、Text、Image、Video、Path、Nested Timeline の bounds は Composition 全体ではなく評価済み外形に一致する。
  - X/Y translate と scale は独立編集でき、uniform lock を明示できる。3D 有効時は Z と X/Y/Z rotation を同じ transform で扱う。
  - Preview 上の Text 編集と SVG/Path editor を既存 production 実装から復旧し、別 surface を作らない。
  - Preview click、drag、gizmo 操作を native HTTP QA で実行し、pixels/state/error log を検証する。

- [ ] **部分実装：Curve Editor/Dope Sheet を本物の keyframe editor にする。** keyframe の文字列表ではなく curve、point、Bezier handle、segment interpolation を描画し、drag で time/value/tangent を編集する。
  - Timeline playhead、Dope Sheet、Curve Editor は同じ stable keyframe ID と local/project time mapping を編集する。
  - step/linear/Bezier/easing、複数 channel、box selection、copy/paste、snap、zoom-to-fit を扱う。
  - Inspector の icon は `automation off`、`enabled/no key here`、`key at current time`、`expression/binding` を区別し、状態に合う tooltip と action を持つ。

- [ ] **部分実装：production Node Editor を唯一の Module editor にする。** node body、typed ports、connection、context menu、property widgets、selection、pan/zoom/grid を既存 surface で維持する。
  - [x] wire paint/hit/handle/reconnect は一つの `WireInteractionGeometry` を使い、unit test と targeted native QA の根拠がある。
  - Output terminal は一 Module に正確に一つ必須で、通常 node catalog から増減できない。Image/Audio 等は同じ Output terminal の typed input であり、別の不自然な `Image Output` node を作らない。
  - Text を含む実行可能 node が右クリック検索に現れ、label が切れず、数値/enum/color/vector を編集でき、その変更が render 結果に効く。
  - asset/Timeline source を graph へ drop すると Published Media Input を作成/接続し、外部 item を内部 Node 群へ展開しない。
  - Node Clip、Attachment、Particle はすべて同じ production Node Editor document を開く。

- [ ] **部分実装：Inspector と Effect/Ensemble を production parity へ戻す。** 既存の numeric drag、vector、color、property-mode widget を共通利用し、label を左揃えに統一する。
  - 複数 Effect を全件表示し、Before Transform / After Transform 等の stage を越えて drag reorder でき、drag 中に insertion preview を出す。
  - enable/bypass、上下/drag handle、delete は意味のある icon と tooltip/context menu を使い、`...` や場当たり的ボタンに隠さない。
  - Blur の大きい sigma で UI を固めず、Tile は centered origin と Offset X/Y を持つ。`mosaic`、`diagonal_clip` 等は Project linear RGBAF32 と宣言した color boundary を正しく変換する。
  - Text Ensemble の既存機能、target、X/Y translate/scale、rotation、stagger/easing を復旧し、Timeline/Node Clip の同じ値へ反映する。

- [ ] **部分実装：panel/window UX を production parity へ戻す。** View menu から任意 panel を開閉でき、dock/float/reorder/resize を保存できる。Beginner/Edit/Motion/Audio/Data/Logic/Diagnostics は機能モードではなく初期 panel layout preset とし、preset 適用後も自由に並べ替えられる。

- [ ] **部分実装：dialog を共通 primitive へ統一する。** Settings、Unsaved Changes、Export、Plugin trust 等は共通 modal、body constraint、footer/action layout を使う。内容に応じて毎 frame サイズが伸びず、Discard/Cancel/Save/close が main thread を freeze せず、1 action が一度だけ dispatch される。

## M3: カット編集と第一級 Transition

- [ ] **未実装：Timeline edit transaction と tool contract を ADR 化する。** selection、linked AV、sync lock、track lock、target track、snap、ripple scope、overlap policy、one-undo boundary を定義する。preview projection と commit が同じ pure edit plan を使い、drag preview と確定結果をずらさない。依存: M1、M2。

- [ ] **未実装：高速カット編集 tool set を実装する。** Select/Move、Razor/Split、Ripple Trim、Ripple Delete、Roll、Slip、Slide、Rate Stretch、Insert、Overwrite、Lift/Extract を icon、keyboard shortcut、context menu で提供する。
  - Ripple は対象 edit point より後の scoped Clip/Marker/Transition を一つの transaction で詰め/送る。
  - Roll は全体長を変えず両隣の out/in を動かし、Slip は配置を変えず source range、Slide は Clip 長を変えず両隣境界を動かす。
  - drag 中に affected Clip 全体の位置、trim frame、gap/overlap、snap guide を live preview する。
  - linked Audio/Video と sync lock を尊重し、無関係な上段 Clip を動かさない。
  - J/K/L、frame step、jump-to-edit、in/out、loop と keyboard-only trim を native QA する。

- [ ] **部分実装：Transition を Timeline の第一級モデルとして実装する。** Timeline 所有の型付き authoring model、atomic edit API、validation、局所 invalidation、階層型 RenderPlan contract は実装済み。UI、実レンダリング、source handle 診断、編集 tool 連携、golden/native QA が未完了であり、まだ利用可能な完成機能とは数えない。
  - Transition は `from_item`、`to_item`、edit point/interval、duration/alignment、processor reference、parameters/automation を Timeline が所有する。
  - 使い方は Transition preset を二 Clip の edit point へ drag/drop、または edit point の右クリック `Add Transition` とする。Timeline 上の handle で duration/alignment、Inspector/Curve Editor で値/easing を編集する。場当たり的な上部ボタンは追加しない。
  - Cross Dissolve、Dip to Color、Wipe、Audio Crossfade を built-in baseline とし、同じ typed transition contract を plugin から追加できる。
  - RenderPlan は from/to の二入力と normalized progress を一 invocation に compile し、Clip を Node 化しない。
  - trim、ripple、roll、split、delete が Transition を deterministic に preserve/resize/remove し、dangling reference を作らない。
  - adjacent、intentional overlap、Nested Timeline、different frame/audio formats、reverse/rate-stretch を golden image/audio と native QA で検証する。

## M4: Audio、音楽時間、MIDI、DTM

- [ ] **部分実装：Audio Output と playback を一つの runtime に統合する。** protected Module Output の visible `Audio` input と Node Clip Audio の targeted native playback QA は存在するが、generic Module audio evaluation は未完成。
  - UI 用語は `Sound` ではなく `Audio` に統一し、pre-v1 の内部 ID/型も一度だけ整理する。
  - Media、Audio Mix、Effect/Instrument Module、Nested Timeline、embedded Video audio、Attachment、published Audio input を同じ RenderPlan audio route/effective-value evaluator で処理する。
  - image evaluation は Audio binding を含んでも失敗せず、Image+Audio を一つの Node Clip が同時に出力できる。
  - waveform cache と playback session を再利用し、短い playback window ごとに RenderPlan compile/decode cache を作り直さない。
  - Timeline waveform、mute/solo/gain/pan、scrub、seek、pause/restart と export mix を同じ時間意味論で native QA する。

- [ ] **未実装：musical time model を Timeline の第一級データにする。** ADR で PPQ/tick、seconds/sample/frame 変換、tempo change、time-signature change、swing、absolute-time Clip と musical-time Clip の挙動を固定する。依存: M1。
  - `TempoMap` と `TimeSignatureMap` は途中変更を保持し、bars:beats:ticks と seconds/samples を deterministic に相互変換する。
  - ruler を Timecode / Frames / Bars & Beats で切り替え、tempo/拍子変更点、grid、snap、metronome が同じ map を使う。
  - tempo 変更時に「秒位置固定」と「拍位置固定」の Clip/Marker/automation policy を明示し、黙って音ズレさせない。

- [ ] **未実装：Timeline/Clip Marker を第一級にする。** ruler click/context menu/shortcut で Composition marker、Clip marker、range marker を追加し、名前、色、comment、duration、musical/absolute anchor を編集する。Ripple、tempo change、Nested Timeline、export metadata と marker navigation の挙動をテストする。

- [ ] **未実装：MIDI を end-to-end で扱う。** MIDI asset/clip/track、timestamped note/CC/pitch bend/aftertouch/program/clock、record/import/export、piano roll、velocity、quantize、humanize、loop、automation conversion を実装する。
  - realtime input と file playback は sample-offset 付き event block を同じ scheduler へ渡す。
  - MIDI Note/Clock から Signal envelope と Event action の両方へ Published Interface 経由で接続できる。
  - MIDI clip 数に比例してユーザー向け Node を生成しない。

- [ ] **未実装：VST3 host を plugin boundary として実装する。** scanner/cache、instrument/effect、parameter automation、preset/state blob、MIDI/event input、audio bus、latency compensation、offline render を備える。
  - 不明な VST DLL を UI thread/audio callback へ直接 load せず、scan と実行の crash isolation、timeout、denylist、再起動を設計する。
  - audio callback は allocation、lock、filesystem、logging を行わず、sample accurate event/automation を処理する。
  - editor UI embedding が使えない場合も generic Inspector で全 parameter を編集できる。
  - missing/version-mismatch plugin は authored state を保持し、無音/素通しの選択と診断を明示する。

- [ ] **未実装：高級 DAW と同じ Timeline 上で DTM を成立させる。** Audio/MIDI Track、record arm、input monitoring、metronome/count-in、loop/punch、comping、take lanes、bus/send/return/master、insert chain、automation lanes、freeze/bounce、time stretch/pitch shift、latency compensation、device routing を実装する。
  - video frame、audio sample、MIDI tick を一つの transport と playhead で同期する。
  - realtime playback と offline export が同じ routing/automation result を出す。
  - overload 時は drop/glitch/xrun を計測表示し、UI を固めない。

## M5: core-level extensible plugin kernel

- [ ] **部分実装：現 ABI を capability-oriented kernel へ一般化する。** `ruvie-plugin-api` ABI v1、manifest/discovery、Property、Style、Decorator、Effector、CPU RGBA8 Effect/Loader は実装済みだが、Exporter、GPU、Audio、MIDI、Node、job/provider、panel は未対応。
  - Plugin Manifest は stable plugin/component ID、semantic version、ABI range、capabilities、permissions、platform binary、optional worker/UI resources を宣言する。
  - Host は capability negotiation を行い、未対応 capability を load 成功に見せない。
  - Project は plugin ID/version/state/provenance を保存し、Rust trait object、DLL pointer、GPU handle を保存しない。

- [ ] **部分実装：core capability extension interface を定義する。** `docs/adr/0006-plugin-extension-kernel.md` で所有権、capability、transport、lifecycle、failure semantics を固定済み。Plugin が Importer/Decoder/Encoder/Exporter、Effect/Transition、Generator/DataSource、Analyzer、ASR/TTS、Audio Processor/Instrument、MIDI Processor、Module Node、GPU kernel/material、command/tool、panel schema、background job を追加できる typed contract を ABI と runtime に実装する。
  - Plugin は immutable Project snapshot と bounded Host Services を受け、直接 Project を変更せず、validated edit proposal/asset result を返す。
  - Core の transaction service が proposal を適用し、selection、validation、Undo/Redo、dirty state を一括管理する。
  - Timeline placement/hierarchy/time、Module published boundary、RenderPlan scheduling/cache dependency は Core の owner のままにする。
  - hot path は versioned typed/batched ABI または shared buffer を使い、frame/audio/particle を一要素ずつ JSON で渡さない。

- [ ] **未実装：plugin process、trust、resource policy を実装する。** Native in-process plugin は trusted のみとし、外部モデル、cloud provider、VST3、AE compatibility host、未知 decoder は原則 worker process へ隔離する。
  - signature/trust prompt、permission（file/network/GPU/audio device/model download）、memory/time/output limits、cancellation/progress、crash recovery、structured diagnostics を提供する。
  - background job は project close/Undo/redo/retry に耐え、古い result を勝手に Timeline へ挿入しない。
  - plugin 更新、missing plugin、state upgrade は pre-v1 の Core project compatibility code と混同せず、component contract 単位で扱う。

- [ ] **未実装：plugin SDK と conformance suite を提供する。** Rust/C header、fixture host、sample plugin、ABI fuzz/property test、malformed buffer、panic/crash/timeout、color/audio format、determinism、offline/realtime parity を検査する。plugin から追加した UI/action も native HTTP QA metadata を登録できるが、QA bridge の任意操作権は与えない。

- [ ] **未実装：After Effects effect plugin compatibility host を調査・段階実装する。** Adobe SDK の effect plug-in contract と配布/ライセンス条件を ADR に記録し、「すべての AE plugin 対応」とは表示しない。公式入口: <https://developer.adobe.com/after-effects/>。
  - Phase 1 は対応 parameter と CPU 8/16/32-bpc pixel effect の明示 subset、color/premultiplication/time contract、out-of-process crash isolation を対象にする。
  - SmartFX、GPU、multi-frame rendering、custom UI の対応可否を capability matrix で検出する。
  - AEGP、AEIO、general extension は effect API と別物として扱い、未対応 binary を load しない。
  - SDK sample または配布許可のある fixture で pixels、parameter、seek、export、crash recovery を検証する。

## M6: 字幕、ゆっくり、生成、Data workflow plugin

- [ ] **未実装：Caption/Transcript を Core Timeline model に追加する。** caption track/item、speaker、text、source time range、Timeline time range、word/phoneme timing、style/template reference、language、provenance を所有する。batch edit、search/replace、line breaking、safe area、burn-in/sidecar export、ripple/split/retime を Node Editor なしで行える。

- [ ] **未実装：local Whisper-compatible ASR plugin を実装する。** local model の明示 download/select/delete、CPU/GPU backend、language/VAD/diarization option、progress/cancel/resume、segment/word timestamp、model/version provenance を提供する。
  - Plugin は Transcript proposal を返し、Core が一 transaction で Caption Track に適用する。
  - 再解析時は stable segment key と manual edit を突合し、手修正を黙って上書きしない。
  - Network を要求せず、長尺素材でも UI thread を block せず、同じ model/settings/input で deterministic な time mapping を得る。

- [ ] **未実装：ゆっくり動画制作 workflow plugin を実装する。** script table（speaker/text/voice/caption/character pose）、一括読み上げ、字幕生成、間の調整、口パク/瞬き cue、立ち絵配置、BGM/SE template を一つの panel workflow として提供する。
  - Core Caption、Audio、Timeline edit transaction、Template、GeneratedItem/Override を使用し、専用の別 Timeline model を作らない。
  - script 行と生成 Audio/Caption/character Clip を stable key で結び、文章修正後も手動位置/見た目 override を維持する。

- [ ] **未実装：PSD/PSDTool 系 layered-character plugin を実装する。** PSD layer/group/mask/name/path を stable key 付き Nested Timeline/asset として読み、表情、口、目、差分の rule mapping を編集/保存する。source PSD 更新時は GeneratedItem/Override reconciliation を行い、消えた layer を Orphaned として提示する。特定ツール固有連携は公開仕様とライセンスを確認して別 capability にする。

- [ ] **未実装：TTS/音声合成 plugin contract と実装を追加する。** local/remote engine の voice/style/dictionary、phoneme timing、prosody、preview、batch render、cache、license/provenance を扱う。生成 Audio と Caption/口パク timing を stable source key で返し、credential/network/cost は permission UI で明示する。

- [ ] **未実装：MiniMax 動画生成 provider plugin を実装する。** prompt/reference image/video/seed/model/settings、credential、見積り/同意、async submit/poll/cancel/retry、result download、provenance を provider capability 上に実装する。結果は Asset として受け取り、Core がユーザー確認後に Timeline へ配置する。provider API を Core や Project model へ直接埋め込まない。

- [ ] **未実装：再利用 Template system を完成する。** title/caption/character/motion/logo/effect/transition/particle/mixer/routing/workspace template を分類し、instance-local text/color/position が sibling を変えないようにする。shared Definition 編集は `この変更は N 個の Instance に反映` を明示し、copy-on-write を既定にする。

- [ ] **未実装：DataSource/Infographics plugin flow を実装する。** CSV/JSON/Table 等を stable row key で Generator に渡し、GeneratedItem と manual Override を作る。reload、row delete/split/key change の Active/Orphaned/Conflict UI と、position/color/text の一件だけの手修正保持を native QA する。

## M7: 統合 3D SceneRuntime、Camera、FBX、Particle、Plexus

- [ ] **部分実装：一つの GPU SceneRuntime に統合する。** `docs/adr/0003-stateful-gpu-scene-runtime.md` と OpenGL 4.3 compute/SSBO の Particle slice は存在する。別の 3D renderer/device を増やさず、既存 `SceneRuntime` を Model/Particle/Plexus 共通 scene pass owner へ一般化する。
  - Skia は 2D/text/vector/effect/composite を継続し、3D pass は color/depth/object-id target を生成して同じ Composition に合成する。
  - GL/Ganesh state、barrier、resource lifetime、device loss を一境界で管理し、Preview と Export が同じ RenderPlan command を使う。
  - scene source が複数でも Composition ごとの camera/depth 関係を保ち、Clip ごとに独立 flatten しない。

- [ ] **部分実装：GPU Particle の最初の executable slice を製品機能にする。** Emitter → Initialize → Gravity → Drag → Sprite Renderer → Output、fixed 1/120 s、seed、checkpoint、最大 capacity、per-instance state、shared compiled pipeline は存在する。
  - Assets/context menu から Particle Node Clip を正式に作成し、curated Inspector と同じ Definition を production Node Editor で編集する。
  - simulation parameter の Timeline keyframe/expression/published input を step boundary で deterministic に sample する。
  - Point、Box/Sphere/Mesh emitter、velocity/lifetime/size/color over life、Vortex/Field/Turbulence、collision、sprite/mesh/ribbon renderer を同じ typed `ParticleSystem` graph と runtime に追加する。
  - real GPU native QA、seek/reverse/repeat/export parity、OOM/device unsupported diagnostics、60 fps benchmark を通す。

- [ ] **未実装：Plexus-style proximity geometry を Particle/point-cloud の上に実装する。** 別 simulator を作らず、GPU spatial hash/grid、neighbor search、max distance/max neighbors、stable edge identity と Point/Line/Triangle renderer node を追加する。
  - animated particle/model vertices を入力でき、distance/color/width/opacity を Published Parameter と Timeline keyframe で制御できる。
  - O(n^2) 全探索を避け、capacity と generated edge/triangle 数に hard limit、diagnostic、cache key を持つ。
  - deterministic seek と Preview/Export parity を golden/native GPU QA する。

- [ ] **未実装：Timeline 3D transform と Camera Item を実装する。** `docs/adr/0004-timeline-3d-space-and-camera.md` は契約のみで、利用可能機能ではない。
  - 2D/3D 共通 transform に position XYZ、anchor XYZ、scale XYZ、rotation XYZ、documented rotation order、parent matrix を持たせる。
  - Camera は interval/layer、perspective/orthographic、FOV/focal length、near/far/focus を持つ第一級 Timeline source とし、camera cut を通常の edit と keyframe で扱う。
  - Preview gizmo、bounds、picking は object-id/depth/evaluated geometry に一致し、Timeline/Dope Sheet/Curve Editor/Inspector が同じ property ID を編集する。
  - Nested Timeline は既定で flatten、明示時だけ 3D space を expose/collapse し、外側移動で内部 local animation を壊さない。

- [ ] **部分実装：FBX/model import を統合 3D pipeline へ接続する。** FBX resource parser の基礎コードはあるが、Timeline 上で描画できる 3D asset pipeline は未完成。
  - mesh、hierarchy、transform、material/texture、camera、light、animation/take、unit/axis conversion を import report 付きで扱う。
  - Asset preview、drag-to-Timeline、Model Node Clip input、Inspector、missing texture relink を実装する。
  - malformed/untrusted file limits、stable asset fingerprint、decode/cache、object picking、color management を検証する。
  - FBX 固有 decode と scene-neutral model representation を分離し、将来 glTF 等も同じ renderer を使えるようにする。

- [ ] **未実装：Motion Logo 向け 2D/3D motion primitive を揃える。** Shape/Text、SVG path edit、Mask/Matte、parent、anchor、constraint、path animation、Text Animator、curve/easing、Nested Timeline と `Fixed/Scale/Loop/Responsive` duration policy を Timeline から編集できる。Node は再利用ロジックの追加に限る。

## M8: 60 fps、cache、export、publish、QA

- [ ] **部分実装：realtime frame budget を守る。** VJ 用 reference scene/hardware を M0 baseline で固定し、release build の定常 playback で 60 fps、p95 frame work 16.67 ms 以下、10 分間の dropped frame 1% 未満、UI main-thread stall 50 ms 未満を目標/CI perf threshold にする。
  - render/decode/audio/plugin/model job を UI thread から分離し、最新 frame 優先の bounded queue と cancellation/backpressure を使う。
  - blur、thumbnail、waveform、video decode、Node zoom、dialog、plugin scan が無制限 work/memory を発生させない。
  - preview resolution/quality degradation はユーザーに表示し、Export の意味論を変えない。

- [ ] **部分実装：cache と incremental scheduling を全 media に統一する。** RenderPlan、frame、decoded media、waveform、Module executable、scene pipeline/state/checkpoint、ASR/TTS/generated asset を共通 dependency/fingerprint policy で invalidation する。cache owner を二重化せず、budget、LRU、metrics、diagnostic を一箇所で追跡する。

- [ ] **部分実装：Preview/Audio/Export parity を保証する。** Project linear RGBAF32 と encoded sRGBA8/plugin boundary、alpha、HDR/SDR、sample rate/channel layout、frame/sample rounding を明文化し、mosaic/diagonal_clip のような format mismatch を compile 時に診断/convert する。Preview と Export は同じ derived plan/effect/audio/scene semantics を使う。

- [ ] **部分実装：native HTTP QA suite を完走する。** 各 UI 変更で対象 interaction を loopback bridge から操作し、visible pixels、project state、selection、Undo/Redo、audio counters、QA metadata、error log を検証する。
  - Assets drag、Timeline move/trim/reorder/content zoom、Preview select/gizmo/text/path、Curve drag、Dope Sheet、Node add/connect/reconnect/property、Effect reorder、Ensemble、Audio playback、Unsaved dialog、Transition、Ripple を scenario 化する。
  - `python scripts/qa-runner.py --mode full --jobs 1` が clean release-like build で通り、panic/render/plugin error が 0 件になる。

- [ ] **部分実装：release/publish を繰り返し可能にする。** `cargo xtask bootstrap` 後は system Python や `RUVIE_PYTHON_HOME` なしで `cargo build --release` と直接 `app.exe` 起動ができ、Python runtime/model/plugin resource は app distribution 内から解決する。
  - `cargo xtask publish` が install を毎回やり直さず cache を再利用し、ready-to-run directory、manifest、licenses、plugin SDK/runtime を生成する。
  - archive は明示 command のみで作り、publish の副作用にしない。
  - clean Windows machine で direct launch、open/save/import/play/export/plugin scan を smoke test し、portable Rust/shell task だけを repository に置く。

## M9: ペルソナ別 E2E 完了条件

- [ ] **未実装：次の一つの acceptance matrix を全て自動/手動 QA で満たす。** 各行を別モデルや別モードで実装せず、同じ Timeline、transport、property、Module、plugin contract を使う。workspace は panel layout preset にすぎない。

| Persona | Node を開かず完了できる主シナリオ | 必要なときだけ深く入る追加シナリオ | 合格条件 |
|---|---|---|---|
| 初心者 | Import → drag to Timeline → Cut/Trim → Text → Canvas edit → Fade → BGM → Export | なし | Node/Port/Binding/RenderPlan を見ず一本完成し、基本操作手数が baseline より増えない |
| ゆっくり動画作者 | script → TTS → Caption → 立ち絵/口パク → BGM/SE → 一括修正 | workflow plugin 設定 | script 再生成後も手動 timing/position/style が stable key で残る |
| YouTube 編集者 | Ripple/Split/Trim/Snap → Transcript/字幕 → B-roll → audio mix → title/effect preset | title/effect の Node Clip を任意で編集 | 長尺/大量字幕でも user-facing Node 数が増えず keyboard 中心で編集できる |
| PV/歌みた動画師 | Nested Timeline、parent、Mask/Matte、Text Animator、Dope Sheet/Curve、local time | Audio/MIDI analyzer → Published Parameter | 外側 placement 移動で内部 animation を壊さず、共通デザインと歌詞/位置の instance 差分が共存する |
| VJ | live source/audio/MIDI → realtime effect/particle/scene → output | bounded procedural Module と routing | 定義済み reference hardware/scene で 60 fps budget を満たし、入力変化で UI/render が停止しない |
| Motion Logo 制作者 | Shape/Text/Path/Anchor/Parent/Camera → keyframe/curve → reusable responsive template | procedural motion Module | `Fixed/Scale/Loop/Responsive` と Intro/Hold/Outro を Node なしで再利用できる |
| Infographics 制作者 | CSV/JSON/Table → stable key → GeneratedItem → layout → one-item adjustment | generator Module/DataSource plugin | data reload 後も manual override が残り、消滅/競合は Orphaned/Conflict として解決できる |
| DTMer | record/edit Audio/MIDI → VST3 → mixer/automation → master/export | audio/MIDI processing Module | sample-accurate transport、latency compensation、realtime/offline parity が成立する |
| ボカロ producer | 作曲/MIDI/Audio と歌詞/PV/Camera/Particle を同じ Timeline で同期編集 | MIDI/Event/Signal で映像 Module を制御 | tempo/拍子変更を跨いでも音、歌詞、映像、marker が同じ playhead で同期する |

最終完了条件は、上表が green であり、かつ常に次が成立することです。

```text
Timeline の構造的複雑さ != ユーザー向け Node 数
```

機能判断に迷った場合は、まず「Node なしで自然に使えるか」を判定し、Node は Timeline の代用品ではなく能力を追加する bounded logic layer として実装します。
