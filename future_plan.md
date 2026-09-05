# RuViE 実装バックログ

この文書は RuViE の今後の実装作業を管理する唯一のバックログです。
新しい機能案、調査結果、受入条件はここへ統合し、別の並行ロードマップを作りません。
「直近の実装順」を実行キューとして扱い、各 slice に記した依存を満たしてから着手します。milestone 番号だけを理由に、独立して閉じられる安全修正や回帰 gate を待たせません。コードと対象テストを通し、UI に影響する項目では native HTTP QA とエラーログ確認まで完了して初めてチェックを付けます。
作業 goal は既存の全体目標とこのバックログを維持し、依存が解決済みの未完了項目を検証可能な vertical slice ごとに実装します。小さな slice の完了を全体 goal の完了に置き換えず、対応する証拠を確認した commit でこの文書の状態を更新します。

## ステータスの読み方

- `[x]` **完了**：現在のリポジトリに実装と相応の自動テストまたは native QA の根拠がある。
- `[ ]` **部分実装**：根拠となるコードはあるが、同じ項目に列挙した不足または最終 QA が残っている。
- `[ ]` **未実装**：宣言、ADR、カタログ項目だけの場合を含み、利用可能な機能としては扱わない。
- 完了状態は 2026-09-06 時点のコードと監査結果に基づく。
  存在する型名やメニュー名だけで完了とは判定しない。

## 変更不能の設計原則

- Timeline は時間配置、Track、Clip、レイヤー順、親子関係、Nested Timeline、直接編集、Keyframe、Mask、Matte、Transition、Marker、Tempo を所有する。
- Node Editor は、明示的な Node Clip または Module Attachment の内側にある有限の処理グラフだけを編集する。通常 Clip や Timeline 全体を Node へ展開しない。
- 外部接続は Published Parameter、Signal、Event、Action、Media Port のみを参照し、Module 内部 Node UUID を参照しない。
- RenderPlan は階層を維持した派生データであり、Project の source of truth にせず、ユーザーにも編集させない。
- プラグインは機能を追加できるが、Timeline の所有権、Project 検証、Undo/Redo、永続化、実行スケジューリングを迂回できない。
- 初心者向け操作に Node、Port、Binding、RenderPlan という語を出さない。高度化によって基本操作の手数を増やさない。
- pre-v1 のため、廃止モデル向け reader、writer、migrator、双方向同期、互換 evaluator を追加しない。
- Project/ABI/component の version field は契約を識別するためのものであり、旧 Project 変換、fallback、dual-write、互換分岐を実装する許可ではない。
- 既存 production 実装を実際の責務境界で拡張する。コピー、別名実装、薄い adapter による二重化を禁止する。
- UI、model、service、runtime、renderer、persistence、test、tooling の全層で DRY を守り、各 source/test/QA ファイルは 1,000 行未満に保つ。
- 一つの検証可能な vertical slice ごとに、対象 unit test、`cargo check`、`git diff --check`、必要な native HTTP QA を通して main へ commit/push する。milestone 完了時は復旧 tag を追加する。

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
M8 Realtime performance / export / publish / QA
  = M2 から M7 の各 vertical slice に並走する継続 gate
M3 + M4 + M5 + M6 + M7 + M8
  -> M9 Persona acceptance
```

M4、M6、M7 は M1 と各契約が固まった後に並行してよいが、M2 の基本編集を壊した状態で先へ進めません。

## 直近の実装順

### 現行 UI の退行復旧（2026-09-05、新機能より優先）

- [ ] Node 本体・header の右クリックから削除・状態編集を使え、Delete/Backspace が縮小表示でも動くようにする。既存 Snarl の node-menu hook と共通 interaction を拡張し、別の context-menu surface を作らない。必須 Output/host boundary のみ保護し、その理由を表示する。通常 Node と接続の削除、一回 Undo、公開 parameter/instance override がある場合の整合性を service と native HTTP QA で検証する。
  - [x] 削除 service を選択単位の一つの transaction にし、Published Interface、instance override、Timeline automation、media binding の依存整理を共通化した。private/COW/shared、複数 Node の Undo/Redo、必須 Output を含む不正な batch の原子的拒否を 6 件で検証し、`3526bfc` を main へ push 済み。
  - UI の追加検証は `target/qa-runs/20260906T-node-batch-overview/node-editor` で PASS。body 右クリック、Delete、公開 parameter を持つ Source の削除、複数選択の一回 Undo、Output 保護、縮小 overview の Backspace を含む。UI 側の統合変更は後続 commit の gate に残す。
- [ ] 選択 edge の通常描画・ハイライト・hit-test・切断・再接続 handle を共通 wire geometry へ統一し、拡大縮小後も実画面上で一致することを確認する。
- [ ] Track header の Eye で映像の表示を切り替え、Audio は維持する。header drag では展開中の Clip/property を含む Track block の実配置を押下中から予告し、release 時だけ一回の並べ替えを commit する。Clip の時間配置不変、Escape、Undo、画素の変化を確認する。
- [ ] Assets と Timeline の footer は共通 panel allocation を使い、panel 外への漏れ・不要な scrollbar・縦位置の不揃いを修正する。Preview toolbar も既存 tool 群を整列し、頂点種類は右クリックから選べるようにする。
- [ ] Text tool は未選択でも有効にし、Canvas 上の既存 Text をクリックすれば編集し、それ以外はその位置へ新規 Text を作る。Content は別枠専用UIではなく既存 property row に統合し、Source と authored property に二重保存しない。
- [ ] Path/Vector は既存正本を拡張し、線分への頂点追加、Pen 新規描画、Rectangle/Ellipse の drag 作成、頂点の Corner/Smooth/Symmetric を右クリックで編集する。既存 Path の移動だけで Illustrator 相当の完成扱いにせず、M2 の全 Vector 要件を継続する。
- [ ] Ensemble Tracking（文字間隔）を bundled descriptor と共通 runtime へ追加し、Target・keyframe・明示 Node Clip 化前後・実画素を検証する。Step Delay は clip-local time の描画テストだけで修正済みとせず、native UI で発生条件を再現して解消する。
  - Step Delay は実 UI で Duration を 0.2→1.5 秒へ変更し、local 0.7667 秒の有効/削除/Undo と local 2.1667 秒の完了状態を実画素で確認した（`target/qa-runs/step-delay-native-r4`）。不動作は再現していない。一方、neutral Ensemble と空 stack の文字描画差（文字単位描画と SkParagraph の差）は残っており、別の描画回帰として解消する。
- [ ] Drop Shadow が文字本体の上へ描かれる不具合を修正する。Shape / Text / Ensemble Text 共通で影・外側光彩を背面、本体を中間、Overlay / Inner 系を前面に描く。同一 phase 内の順序を維持し、後続文字の影が先行文字を覆わないこと、角度 120° の右下方向、透明度、Node Clip 化前後を実画素と native QA で確認する。
- [ ] 共通数値スクラブは欄の外で pointer を離しても、capture した control だけを一度確定する。Appearance Distance の 28 px drag が一時値と Preview だけを変え、履歴に保存されない不具合を再現済み。scalar/vector/timing/Paint が同じ確定処理を使い、実 UI と Undo まで検証する。
- [x] 共通 Color Picker / Palette のドラッグが背後の Node 移動・接続・canvas pan/zoom へ漏れないようにした。背後の Output header と hue control を重ねて popup 外へドラッグし、色だけが変わり Node 位置・選択・pan/zoom が変わらないことを native QA で確認した（`target/qa-runs/20260905T-popup-drag-2/color-palette`）。共通 Node surface / viewport の回帰テストも追加した。
- [x] Inspector の空白での通常左ドラッグによるスクロールを無効化した。ホイールとスクロールバーは維持し、native QA で実際にホイールでスクロールした後の空白ドラッグが offset を変えないことと数値・色編集を確認した（`target/qa-runs/20260905T-scroll-4/inspector-source`）。
- [x] Timeline Clip の両端 trim を復旧した。drag threshold 通過後の座標ではなく press origin から移動／左右 trim を分類する。狭い Clip と画面外の端の単体テストに加え、両端の長さ変更・兄弟 Clip 不変・描画 geometry・Undo を native QA で確認した（`target/qa-runs/20260905T-trim-scroll/timeline-edit`）。
- [ ] Text の明示 Node Clip 化前後で Content / Font / Font Size / Fill と Ensemble の編集能力を維持する。共通 descriptor と元の Ensemble UI を使い、認識可能な Text chain では構造操作を一つの graph transaction / Undo にする。
- [ ] Node Editor に Assets の Image / Video / Audio を drag-and-drop で追加し、Asset identity と既存 media factory/runtime を共有する。
- [ ] Node parameter の時計から Timeline 所有の keyframe を編集できるようにし、Inspector / Curve Editor と同じ automation を表示・編集する。
- [x] Node header の enabled / bypass 操作を復旧し、状態表示だけのチェックマークにしない。native QA で bypass による画素変化と resume 後の元画素への一致を確認した。
- [ ] Node header 全域の drag、選択しても動かない pin geometry、接続中 wire preview、marquee selection rectangle を共通 Node Editor surface で修正する。
- [x] Edge の右クリック切断メニュー、選択後 Delete/Backspace、Ctrl+右ドラッグ切断と Alt+右ドラッグ接続を共通 Node Editor の実画面操作で検証した。Blender Node Wrangler 全機能の互換実装ではない。
- [ ] 描画遅延を production renderer の CPU/GPU 計測で切り分け、通常 Text/Shape/SkSL の不要な全画面 readback/reupload を除去する。画の一致、実解像度での改善量、残る terminal color / UI upload コストを確認し、未達の 60 fps を達成扱いにしない。
  - [x] ProjectColorPipeline が検証した完全な terminal transform chain を既存 Renderer の GPU owner 内で実行し、Project working RGBAF32 の全画面 readback を最終 RGBA8 のみにした。builtin の式・定数・processor identity を CPU と共有し、alpha/extended range/nonfinite 検査、複数 stage 順序、resize/context lifetime を実 GPU parity test 8件と color-management 60件で検証した。backend が完全な GPU chain を提供しない場合は同じ CPU processor を使う。`5b832b1` を main へ push 済み。Full-HD 4-Solid は terminal 含め 5.34 ms、製品全体の 60 fps 証明ではない（`docs/performance/gpu-terminal-after-2026-09-05.json`）。
- [x] Timeline の複数 Clip 選択と Track 間 drag 移動を復旧した。複数 Clip の一括移動と一回の Undo を native QA で検証した。
- [ ] Solid の色、Rectangle / Ellipse の geometry、Shape style を既存の Inspector / property / style component へ接続する。
  - [x] Solid の色、Rectangle / Ellipse の Width / Height / Fill を共有 property row で編集し、表示画素・Undo/Redo を native QA で確認した。通常 Clip の値と keyframe を明示 Node Clip 変換へ引き継ぐ。primitive geometry は SourceRef と Module runtime が同じ実装を使用する。
  - [ ] Stroke や複数 Appearance style の完全な編集、Timeline-owned expression の Node Clip への引継ぎを完成する。現行 Published Parameter に表現できない expression の変換は原子的に拒否し、黙って固定値にしない。
- [ ] SkSL Shader Clip の直接作成導線を復旧する。既存 Shader converter と有限 Node Clip を使い、別 renderer を作らない。
- [ ] 新規 Clip の position と anchor を中心に初期化する。既存配置・手動編集・keyframe は変更しない。
- [x] Curve Editor の time ruler と共通 playhead scrubbing を復旧し、native QA で Timeline の frame と連動することを確認した。
- [x] 組込 Keyframe の時刻・値・補間パラメータ編集を共有 Modal と補間メニューへ戻した。
- [ ] plugin 補間の選択を追加する。現在の EasingFunction は閉じた列挙型であり、以前から存在した選択 UI の復旧だけでは完了しない。plugin evaluator descriptor と Timeline automation の契約を先に拡張する。
- [x] Effect 追加の検索付き accordion/category menu を共通メニューへ戻した。カテゴリを押すと popup が閉じる問題も修正し、検索→指定 stage への追加→描画変化を native QA で確認した。
- [ ] Export 設定 dialog と Exporter plugin 選択を既存 production component から復旧し、選んだ設定を実際の worker へ渡す。
- [ ] 上記を native HTTP の実画面操作、描画、Undo/Redo、保存、出力で回帰検証して main へ反映する。

到達済みの範囲: Particle System は、Assets からの drag、有限な Module Definition、Inspector と production Node Editor、実 GPU Preview/seek、production と同じ renderer semantics を使う Export 専用 session、同一フレームの parameter override と Undo/Redo、project file への保存→app終了→新プロセス起動→再読込みまでの最小対話導線に到達した。通常 Clip と Timeline 全体は Node 化していない。対応 GPU runner での必須化、複数 Particle layer の 60 fps 計測が未完了なので、製品 vertical slice 全体は完了扱いにしない。

直前の完了 gate: release production app の native HTTP QA は Inspector Source を加えた 23/23 が通過した（evidence `target/qa-runs/20260905T132231Z-full-70460`）。Node の切断・再接続・bypass、Timeline 複数選択と Track 間移動、Curve ruler、Source 編集と明示 Node Clip 変換の画素一致、検索付き Effect 追加を実画面で確認した。Export 中の Quit/New/Open は cancel の受理から terminal cleanup まで待ち、staging/Audio cleanup 前に Project や window を破棄しない。単純な Full-HD 4-Solid の描画は 452.62→64.75→5.34 ms に改善したが、UI upload・decode・複雑な Effect/Particle を含む製品全体の 60 fps は未達扱いとする（`docs/performance/rendering-2026-09-05.md`）。

1. **E1: 一般動画 Export を原子的にする。** M8 の `E1` を正本とする。既存 destination の破損と partial output を防ぐ安全要件なので、新しい Transition、Audio、3D 機能より先に閉じる。
2. **T1: Image Transition の配置・handle 編集を完成する。** 実装済みの有限な Transition Module を edit point への drag/drop、右クリック、Timeline handle に結合する。trim/ripple/roll との統合は T2 として分ける。
3. **T5a: Node-authored Transition を既存 production Node Editor で作れるようにする。** T1 の配置 UX を完成後、built-in の `Edit a Copy` または空の有限 Transition Module から A/B/Progress/Output 境界を保った private Definition を作る。Timeline 全体や from/to Clip は Node 化しない。共有 Definition、Template、plugin 化は T5b へ分離する。
4. **A0 → A1: 共通 transport と Media Audio を固める。** waveform、playback、seek、loop、export parity を先に閉じ、generic Module dual-output、Tempo/拍子/Marker、MIDI、VST3、DAW routing は後続 slice とする。
5. **Node catalog の schema ownership を閉じる。** sampling capability、property key、Published Interface 生成規則、hard limit を共通 descriptor の一つの正本へ集約し、Particle node を増やす前に factory/compiler/UI の重複を除去する。
6. **同じ SceneRuntime に 3D 基盤を追加する。** Timeline 3D transform、Camera Item、scene-neutral model、FBX の順に実装し、別 renderer/device や Inspector 専用データモデルを作らない。
7. **Particle の時間入力と表現力を拡張する。** fixed-step parameter schedule を RenderPlan の共通 transport として実装してから、Box/Sphere/Mesh emitter、color/size over life、Field/Turbulence、collision、mesh/ribbon、Plexus へ進む。Inspector 専用の第二モデルや任意 Node UUID binding は作らない。

継続 gate: M2 の production editor 回帰を常にゼロに保つ。追加機能は既存 Assets、Timeline、Preview、Inspector、Node Editor、Curve Editor、共通 property/media picker、pan/zoom/grid、dialog に統合し、専用の並行 UI や重複 resolver を残さない。panel/window UX と複合 viewport scenario は、影響する各 slice と同時に閉じる。復旧 tag を基準に、各主要 surface の golden screenshot、interaction manifest、keyboard/mouse step count、QA metadata を production-parity gate として比較し、既存 surface の置換、基本操作の手数増加、並行簡略 UI の混入を失敗にする。

各項目は途中の型やメニュー項目では完了にせず、該当する production UI、実行結果、対象テストまたは native QA が同じ commit で揃った時点で次へ進みます。authoring state を変更する slice は、保存→終了→別 process で再読込みと Undo/Redo も必須です。

現在は上記 UI 退行復旧と GPU terminal color の検証を進めている。E1 の cancellation/lifecycle は閉じたが、decoded video/audio の Preview/RenderPlan 数値比較などは M8 の E1 に残す。未完了の UI、Audio、3D、plugin、persona acceptance を完了扱いにせず、同じ production owner の次の未完了 slice へ進む。

---

## M0: Baseline、復旧点、リポジトリ構造

- [x] **完了：復旧点を固定する。** `pre-production-ui-reset-20260904` と `recovery/before-production-ui-reset-20260904` が復元点を指す。以後、比較のために production UI の挙動と見た目を参照し、旧 evaluator を本番へ戻さない。

- [x] **完了：Rust workspace package を `crates/` へ集約する。** host が所有する `app`、`library`、`color-management`、`plugin-api`、`python-runtime`、`pan-zoom-ui`、`node-editor-ui`、`xtask` は `crates/<name>` に配置され、root `Cargo.toml` もその配置を参照している。次を満たして完了とする。
  - 旧 root package directory が Git 差分上も削除され、同じ source の二重配置がない。
  - `plugins/<plugin-id>` は独立配布する plugin bundle、`examples/` は外部実装例として workspace package 集約の対象外であることを明文化する。
  - portable script、docs、CI、fixture の manifest path を全て新配置へ直す。
  - clean checkout で `cargo metadata --locked`、workspace build/test、publish が通る。

- [ ] **部分実装：production baseline を記録する。** 復旧 tag と現 main について、project load、first frame、seek、edit-to-preview、連続再生、audio、export、100/1,000/10,000 Clip、同一 Module 多数配置、GPU/CPU/メモリを同じ fixture で計測する。数値、OS、GPU、driver、release profile、fixture hash を `docs/performance/` に保存し、未計測の「速い」を完了条件に使わない。
  - [x] `cargo xtask performance-baseline` から optimized production path を計測し、Project load、100/1,000/10,000 Item、共有 Module 1,000 Instance、first frame、seek、連続 frame、edit-to-preview、Audio cold/cache、PNG export を schema 検証済み JSON として保存できる。
  - [x] OS、CPU、Rust/Git/profile、dirty state、fixture/load/audio SHA、warmup/raw sample を記録し、未計測の GPU/driver/full FFmpeg export は値を捏造せず `null + reason` にする。
  - [ ] 復旧 tag と clean main の同一 machine 比較、GPU Preview、process/GPU memory、完全な動画 export を計測し、CI/定期 perf test の回帰閾値を決める。

- [ ] **部分実装：コード品質の継続ゲートを固定する。** `AGENTS.md` の再利用、共通 surface、DRY、1,000 行制限を CI で検査し、`rg` ベースの境界検査だけでなく Rust dependency graph と source line count も検証する。
  - [x] `scripts/check-source-file-size.sh` を CI の fail-closed quality gate に組み込み、first-party の Rust/Python/shell/JS/TS/C/C++/SkSL を 1,000 行以下に固定した。tracked と non-ignored untracked の両方を NUL-safe に検査する。
  - [ ] 重複 owner、恒久的な `new`/`legacy`/`timeline_first`/意味のない `v2`、名称責務、`.ps1` 混入は、機械検査できる範囲を追加し終えるまでレビュー規約だけで完了扱いにしない。
  - 同じ責務の重複 owner がない。
  - `new`、`legacy`、`timeline_first`、意味のない `v2` を恒久 module 名にしない。
  - repository automation に `.ps1` を commit しない。
  - `Node Editor` と `Curve Editor` の名称と責務を混同しない。

- [x] **production app から参照されない公開 `editor::ExportService` を削除した。** pre-v1 の旧 Export coordinator、public re-export、final path への直接書込みを互換 API として温存せず除去した。画素一致は `RenderService`、source-alias と lifecycle は現行 authoring `RenderServer` または責務別 utility の test へ移し、連番 path/template の旧経路専用契約は削除した。`RenderServer` が唯一の production authoring export coordinator である。

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

- [ ] **部分実装：Node catalog を唯一の schema owner にする。** Particle の role、port、descriptor、runtime status は共通 catalog へ集約済み。property key、Published Interface の生成規則、上限値を factory/compiler/UI に再定義せず descriptor から導出し、sampling/automation capability は authoring 固有型ではなく catalog/common が所有する。
  - authoring validation も選択 Output の reachability を共通規則から導出し、到達不能な required Published Media Input に不要な binding を要求しないようにする。現在は persisted binding の構造整合性を保つため、dead input にも binding が必要である。

- [ ] **部分実装：Signal/Event binding runtime を完成する。** `docs/adr/0002-event-runtime.md` は public contract のみ完了している。
  - Signal は base、keyframe、各 binding、manual override の寄与と operator/smoothing を deterministic に合成する。
  - Inspector は effective-value provenance を表示し、Canvas で直接操作しても binding を黙って切らない。
  - Event は `Restart`、`IgnoreWhilePlaying`、`Queue`、`Overlap` を実装し、`Overlap` は永続 Item の clock 書換えではなく bounded reactive instance を生成する。
  - 同じ Definition の複数 Nested placement の片方だけを `InstancePath` で制御できる。

- [ ] **未実装：GeneratedItem/Override を第一級にする。** generator/source row の stable key、generator version、provenance と manual patch を保存し、再生成で同じ item の修正を維持する。消滅/競合時は `Active`、`Orphaned`、`Conflict` を黙って捨てず、解決 UI と Undo を提供する。

## M2: production editor surface の復旧と統合

- [ ] **部分実装：共通 viewport を唯一の navigation 実装にする。** Timeline、Curve Editor、Node Editor、Preview は `pan-zoom-ui::CanvasState` と application `ViewportController` を使用し、2026-09-04 時点の既存 native interaction suite は全件通過した。複合 pan/zoom 中の長時間操作、極端な座標、Transition/Ripple/3D を含む未追加シナリオは残る。
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
  - [x] canonical `PathValue` と同じ Preview surface 上で point/Bezier handle を編集し、Undo/Redo と render refresh へ接続する Path Editor vertical slice、および native HTTP QA を実装した。
  - [ ] Preview 上の Text 直接編集を production 実装から復旧し、別 surface を作らない。
  - [ ] 完全な SVG document の import/edit は Path geometry、group、transform、Paint、Mask、symbol/use、text、external resource の対応範囲を ADR で固定し、既存 Path Editor と Appearance Stack へ段階的に接続する。
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

- [ ] **部分実装：Shape/Text の Appearance を第一級スタックにする。** geometry/text layout と分離した Timeline 所有の operation stack を既存 StylePlugin/descriptor/renderer へ接続し、簡単な装飾のために Node Editor を開かせない。Fill/Stroke の正本・Inspector・明示 Node Clip 変換を実装中。全10種レイヤースタイルと完成条件は次のチェック項目で追跡する。
  - [ ] **レイヤースタイル全10種を Shape と Text に実装する。** [ユーザー指定の一覧](https://321web.link/photoshop-layer-styles/) を対象とし、メニュー登録だけで完了にしない。
    - [ ] ベベルとエンボス：形状由来の高さ/法線、深さ・サイズ・方向・光源 angle/altitude、highlight/shadow、輪郭・texture を共通 mask/lighting 経路で描画する。
    - [ ] 境界線：内側/中央/外側、幅、Solid/Gradient/Pattern Paint、複数追加・順序を同じ Style/paint 正本で扱う。
    - [ ] シャドウ（内側）：形状内部への陰影、角度・距離・choke・size・opacity/blend を実描画する。
    - [ ] 光彩（内側）：edge/center、size・choke・color/paint・opacity/blend を実描画する。
    - [ ] サテン：形状 mask 由来の陰影、angle・distance・size・contour/invert を実描画する。
    - [ ] カラーオーバーレイ：形状の alpha を維持し、managed color・opacity・blend を適用する。
    - [ ] グラデーションオーバーレイ：下記第一級 Gradient Paint を使い、stop・種別・angle/scale/offset・opacity/blend を編集・描画する。
    - [ ] パターンオーバーレイ：下記 Pattern Paint を使い、source・tile/repeat・transform・opacity/blend を編集・描画する。
    - [ ] 光彩（外側）：形状外部への光彩、spread・size・color/paint・opacity/blend を実描画する。
    - [ ] ドロップシャドウ：形状背面への影、angle・distance・spread・size・opacity/blend を実描画する。
    - 10種共通：既存 Style stack から追加・削除・並べ替え・enable、数値/色/paint 編集、keyframe、Undo/Redo、保存再読込み、instance 独立性を通す。shared light と内側/外側の合成意味論を明示し、Style ごとに renderer を複製しない。
    - 共通 alpha mask、blur/morphology、lighting、Paint、managed color の owner を使い、stroke/glow/shadow が Clip/Composition bounds で欠けないようにする。Inspector/Node で parameter の効果が異ならず、明示変換前後、Preview/Export の画素と timing を golden と native QA で確認する。
  - [x] **Slice A：Solid Project Palette の基盤。** Project 所有の `ProjectPalette`、stable `PaintDefinitionId`、Solid `PaintDefinition`、厳格な保存/validation、局所 invalidation、Undo/Redo、既存 Color picker 内の `Picker / Palette`、追加・copy適用・rename・drag reorder・削除を実装した。Inspector と production Node Editor は同じ picker を使い、managed `ColorValue` の f64/色空間を保持したまま適用する。通常 Clip は Node へ展開しない。通常 Shape/Text の主要 Paint property への統合は Slice B/C/F の完了条件に残す。
  - [ ] **Slice B：authoring color を managed `ColorValue` へ統一する。** encoded-sRGBA8 境界用の旧 `PropertyValue::Color` は不可逆変換を避けるため Palette 対象外とする。同じ変更で旧 authoring color variant を置換・削除し、通常 Shape/Text を同じ Palette 導線へ接続する。reader、migrator、fallback、dual-write は追加しない。
  - [ ] **Slice C：Gradient を第一級 Paint として実装する。** Fill/Stroke の見た目を `Color` へ押し込めず、`Paint = Solid | Gradient | Pattern` として型付けする。Paint 対応の Fill/Stroke は既存 color 導線を拡張した一つの共通 `PaintPicker` から Solid/Gradient/Pattern を選び、色だけを受ける parameter は `ColorValue` のままとする。
    - `Gradient` は stable ID、Linear/Radial/Conic/Freeform 種別、stable stop ID、position、managed color、opacity、midpoint/interpolation、spread、gradient transform を保持する。
    - stop の追加・削除・並べ替え、Canvas 上の方向/中心/半径編集、Inspector/Curve Editor の automation を同じ値へ接続する。
    - 通常編集はその Appearance instance だけを変更する。補間と composite は Project working-linear color space で行い、Preview/Export の terminal 変換と混同しない。
  - [ ] **Slice D：Pattern Paint を実装する。** Image/vector/procedural source、tile/repeat mode、origin、scale、rotation、offset、transform、color-space 境界を型付きで定義し、Fill と Stroke の同じ `PaintPicker`、Canvas gizmo、automation、Preview/Export へ接続する。型だけを予約して完成扱いにしない。
  - [ ] **Slice E：Palette の共有とライブラリ機能を完成する。** Solid に加えて Gradient/Pattern を同じ swatch grid へ置き、group/tag、検索、複製、import/export、built-in/user library から Project への取り込みを実装する。色ボタンとは別の一時的な Palette panel や上部ボタンは増やさない。
    - swatch 適用は `Copy` と明示的な `Linked` を区別する。Linked 編集時は影響 instance 数を表示して一 transaction で更新し、missing/削除時も resolved Paint を保持して黙って透明や黒へ変えない。
    - 保存/load、Undo/Redo、linked update、Solid/Gradient 選択、drag reorder、広色域/HDR 値保持を unit test と native HTTP QA で検証する。
  - [ ] **Slice F：`AppearanceStack` を実装する。** 各 `AppearanceEntry` は stable ID、名前、visible/enabled、opacity、blend mode、local transform、対象、operation、parameter/automation を持ち、`Fill`、`Stroke`、`Effect`、`Group` を任意順・任意数で積める。
    - Fill は solid/linear gradient/radial gradient/conic gradient/freeform gradient/pattern を扱う。Stroke も同じ Paint を持ち、width/alignment/join/cap/miter/dash/offset/arrowhead/width profile/brush を段階実装する。Drop Shadow、Inner/Outer Glow、Blur、Offset Path、Roughen、Warp は適用対象を明示する。
    - 対象は `WholeShape | Group(stable_id) | Subpath(stable_id)` とし、Path 編集後も対応を保つ。対象が消えた場合は別要素へ掛けず、orphan/conflict として Inspector に残す。
    - Inspector は既存 Effect Stack の property row、keyframe mode、drag/drop、insertion preview、icon/context menu を共通利用し、複製、group 化、enable/bypass、削除、drag reorder を一つの Undo で行う。
    - Timeline/Dope Sheet/Curve Editor が parameter の時間を所有し、RenderPlan は局所的な Shape branch/style/effect/composite pass へ派生 compile する。Appearance 数に比例したユーザー向け Node は生成しない。
    - 明示的に Shape Clip を Node Clip へ昇格した場合だけ、順序、stable identity、properties、automation、blend を有限の Shape branch + Style/Effect + Merge へ等価変換し、image golden と一回 Undo を検証する。
    - preset と共有 Definition は instance 値と定義編集を区別し、通常の色・線幅変更はその Shape instance だけへ反映する。共有変更時だけ影響 instance 数を明示する。
    - 既存 `DrawStyle`、Fill/Stroke descriptor、semantic Style stack、Effect Stack UI を移植元として使い、旧 Project graph を adapter で並行接続しない。

- [ ] **部分実装：panel/window UX を production parity へ戻す。** View menu から任意 panel を開閉でき、dock/float/reorder/resize を保存できる。Beginner/Edit/Motion/Audio/Data/Logic/Diagnostics は機能モードではなく初期 panel layout preset とし、preset 適用後も自由に並べ替えられる。

- [ ] **部分実装：dialog を共通 primitive へ統一する。** Settings、Unsaved Changes、Export、Plugin trust 等は共通 modal、body constraint、footer/action layout を使う。内容に応じて毎 frame サイズが伸びず、Discard/Cancel/Save/close が main thread を freeze せず、1 action が一度だけ dispatch される。

## M3: カット編集と第一級 Transition

- [x] **完了：Timeline edit transaction と tool contract を ADR 化する。** `docs/adr/0007-timeline-edit-transactions.md` で selection、linked AV、sync lock、track lock、target track、snap、ripple scope、overlap policy、sparse preview projection、stale revision、one-undo boundary、Marker/Transition追従を固定した。実コードは後続項目として段階実装する。依存: M1、M2。

- [ ] **未実装：高速カット編集 tool set を実装する。** Select/Move、Razor/Split、Ripple Trim、Ripple Delete、Roll、Slip、Slide、Rate Stretch、Insert、Overwrite、Lift/Extract を icon、keyboard shortcut、context menu で提供する。
  - Ripple は対象 edit point より後の scoped Clip/Marker/Transition を一つの transaction で詰め/送る。
  - Roll は全体長を変えず両隣の out/in を動かし、Slip は配置を変えず source range、Slide は Clip 長を変えず両隣境界を動かす。
  - drag 中に affected Clip 全体の位置、trim frame、gap/overlap、snap guide を live preview する。
  - linked Audio/Video と sync lock を尊重し、無関係な上段 Clip を動かさない。
  - J/K/L、frame step、jump-to-edit、in/out、loop と keyboard-only trim を native QA する。

- [x] **T0 完了：Timeline 所有の Transition と有限な Transition Module の実行基盤。** Transition の時間と配置は Timeline が所有し、処理だけを built-in、plugin、または有限な Transition Module として差し替える。
  - [x] `from_item`、`to_item`、edit point/interval、duration/alignment、processor reference、parameters/automation を持つ型付き authoring model、atomic edit API、validation、局所 invalidation、階層型 RenderPlan、Image/Audio の実レンダリングを実装した。
  - [x] Image 用の保護された `A: Image`、`B: Image`、`Progress: Number(0..1)` と、一つの共通 Output terminal が持つ `Image` 型入力を境界にした `Transition ModuleDefinition` を実装した。Audio も同じ Output terminal の `Audio` 型入力を使う。共有 Definition は一度だけ compile し、各 Transition は Module Instance、InstancePath、parameter/automation だけを持つ。
  - [x] Transition processor の既存 Module document は、`Edit Transition Logic` を明示したときだけ production Node Editor で開く。from/to Clip や Timeline 全体は Node へ展開しない。この完了は Transition Module の新規 authoring、catalog、Template、plugin 登録の完成を意味しない。
  - [x] Inspector、Dope Sheet、Curve Editor から Published Parameter と automation を編集し、必須追加 input を一 transaction で割り当て、非 Normal Blend Mode を含む実レンダリングと native HTTP QA を通した。

- [ ] **T1: Image Transition の production 配置・handle UX を完成する。** 依存: T0、M2 共通 viewport。Cross Dissolve を canonical Transition browser から二 Clip の edit point へ drag/drop、または edit point の右クリック `Add Transition` で追加する。drag 中は insertion と handle の live preview を表示し、Timeline handle で duration/alignment を編集する。source handle 不足時は Project を変更せず診断する。場当たり的な上部ボタンは追加しない。
  - DoD: 両方の追加導線、handle 編集、一 transaction、一 Undo/Redo、保存→終了→別 process 再読込み、Preview/Export golden 一致、native HTTP QA の pixels/state/error 0 を満たす。
  - Non-goals: Audio Crossfade、plugin/template 登録、未実装の Ripple/Roll、Timeline/Clip の Node 展開。

- [ ] **T2: Transition-aware edit operation を完成する。** 依存: T1 と対象となる M3 edit tool。trim、ripple、roll、split、delete が Transition を deterministic に preserve/resize/remove し、dangling reference を作らない。adjacent、intentional overlap、Nested Timeline、reverse、rate-stretch を transaction preview、golden image、native QA で検証する。

- [ ] **T3: Image Transition の built-in baseline を揃える。** 依存: T1。Dip to Color と Wipe を T0 と同じ typed contract で追加し、異なる image format、alpha、color space、Nested Timeline の Preview/Export parity を検証する。

- [ ] **T4: Audio Crossfade を同じ Transition model に接続する。** 依存: A1。sample boundary、gain curve、rate/reverse、embedded Video audio、realtime/offline parity を検証し、Image Transition 用の別 Timeline model を作らない。

- [ ] **T5a: Node-authored Transition を production Node Editor で作れるようにする。** 依存: T1。built-in Transition の `Edit a Copy` または空の Transition Module から有限な private Definition を一 transaction、一 Undo で作り、node の追加・接続・再接続・parameter 編集・適用を既存 production Node Editor で行う。M5/M6 の plugin/template 基盤を待たず、Project 内 private Module として完成させる。
  - 保護された `A: Image`、`B: Image`、`Progress: Number(0..1)` と、一つの共通 Output terminal は削除・重複不可とする。明示的な auxiliary Published Media/Parameter/Signal だけを追加でき、任意の Timeline traversal や内部 Node UUID binding を許可しない。
  - DoD: edit point への適用、一 transaction、一 Undo/Redo、保存→終了→別 process 再読込み、必須 Output 診断、Preview/Export の image/audio/timing golden、native HTTP QA の pixels/state/error 0 を満たす。Transition の長さ、配置、handle、automation の時間は引き続き Timeline が所有する。

- [ ] **T5b: Node-authored Transition の Template 化、共有 Definition、plugin 登録を完成する。** 依存: T5a、M5 plugin kernel、M6 Template contract。private Definition を検索・再利用・共有・plugin package 化し、共有 Definition 編集前に影響 instance 数を明示する。元の built-in や sibling instance を暗黙に変更せず、copy/shared/plugin 各 provenance と更新診断、昇格前後の golden を検証する。

## M4: Audio、音楽時間、MIDI、DTM

- [ ] **A0: 一つの transport/clock kernel を固定する。** 依存: M1。Project time、video frame、audio sample の整数/rational 変換、play/pause/seek/scrub/loop、block scheduling、device/offline clock の ownership と underrun/cancellation を ADR と executable test にする。UI、Preview、Audio、Export が別々の playhead/clock を持たない。
  - DoD: 境界 frame/sample、長時間、異なる fps/sample rate、loop、seek/restart を property test し、同じ input sequence から同じ timestamped block schedule を得る。
  - Non-goals: MIDI、TempoMap、VST3、mixer routing、Node graph。

- [ ] **A1: Media Audio の waveform、playback、Export を共通 transport に統合する。** 依存: A0、M2 Timeline。Audio asset、embedded Video audio、Nested Timeline を同じ RenderPlan audio route で処理し、waveform cache と playback/decode session を再利用する。短い playback window ごとに RenderPlan compile や decode cache を作り直さない。
  - DoD: move/trim、gain/pan/mute/solo、scrub、seek、pause/restart、loop で playhead、waveform、audio block が一致する。realtime capture と offline export の sample/hash または仕様化した許容差を検証し、編集は一 Undo/Redo と保存→終了→別 process 再読込みを満たす。native HTTP QA は production UI を操作し、audio counter、export artifact、error 0 を確認する。
  - Non-goals: MIDI、VST3、metronome、DAW bus routing、別 Audio Timeline、Clip 数に比例する Node。

- [ ] **A2: generic Module の Image+Audio dual-output と Video+Audio Node Clip 昇格を完成する。** 依存: A0、M1 Node catalog schema ownership。protected Module Output の visible `Audio` input と Node Clip Audio の targeted native playback QA は存在するが、generic Module audio evaluation は未完成。Media、Audio Mix、Effect/Instrument Module、Attachment、published Audio input を同じ evaluator で処理し、image evaluation は Audio binding を含んでも失敗しない。一つの Node Clip が Image+Audio を同時に出力する。

- [ ] **A3: Track/Bus/Master mix を production UI と runtime に統合する。** 依存: A1。基本の gain/pan/mute/solo は Inspector と mixer を主導線にし、Track/Bus/Send/Return/Master、insert chain、meter、automation と latency accounting を同じ RenderPlan route にする。高度な Audio Node/Port は、明示した Clip/Track/Bus/Master Module だけを同じ production Node Editor で編集する。UI 用語と pre-v1 の ID/型は `Audio` に一度だけ統一し、`Sound` の並行名称を残さない。

- [ ] **A4: musical time model を Timeline の第一級データにする。** 依存: A0、M1、M2 共通 viewport、M3 edit transaction。ADR で PPQ/tick、seconds/sample/frame 変換、tempo change、time-signature change、swing、absolute-time Clip と musical-time Clip の挙動を固定する。
  - `TempoMap` と `TimeSignatureMap` は途中変更を保持し、bars:beats:ticks と seconds/samples を deterministic に相互変換する。
  - ruler を Timecode / Frames / Bars & Beats で切り替え、tempo/拍子変更点、grid、snap、metronome が同じ map を使う。
  - tempo 変更時に「秒位置固定」と「拍位置固定」の Clip/Marker/automation policy を明示し、黙って音ズレさせない。

- [ ] **A5: Timeline/Clip Marker を第一級にする。** 依存: M3 edit transaction、A4。ruler click/context menu/shortcut で Composition marker、Clip marker、range marker を追加し、名前、色、comment、duration、musical/absolute anchor を編集する。Ripple、tempo change、Nested Timeline、export metadata と marker navigation の挙動をテストする。

- [ ] **A6: MIDI を end-to-end で扱う。** 依存: A0、A4。MIDI asset/clip/track、timestamped note/CC/pitch bend/aftertouch/program/clock、record/import/export、piano roll、velocity、quantize、humanize、loop、automation conversion を実装する。
  - realtime input と file playback は sample-offset 付き event block を同じ scheduler へ渡す。
  - MIDI Note/Clock から Signal envelope と Event action の両方へ Published Interface 経由で接続できる。
  - MIDI clip 数に比例してユーザー向け Node を生成しない。

- [ ] **A7: VST3 host を plugin boundary として実装する。** 依存: A0、A3、A6、M5 process isolation。scanner/cache、instrument/effect、parameter automation、preset/state blob、MIDI/event input、audio bus、latency compensation、offline render を備える。
  - 不明な VST DLL を UI thread/audio callback へ直接 load せず、scan と実行の crash isolation、timeout、denylist、再起動を設計する。
  - audio callback は allocation、lock、filesystem、logging を行わず、sample accurate event/automation を処理する。
  - editor UI embedding が使えない場合も generic Inspector で全 parameter を編集できる。
  - missing/version-mismatch plugin は authored state を保持し、無音/素通しの選択と診断を明示する。

- [ ] **A8: 高級 DAW と同じ Timeline 上で DTM を成立させる。** A0 から A7 を別 mode/別 Timeline に分岐させず、Audio/MIDI Track、record arm、input monitoring、metronome/count-in、loop/punch、comping、take lanes、bus/send/return/master、insert chain、automation lanes、freeze/bounce、time stretch/pitch shift、latency compensation、device routing を完成する。
  - video frame、audio sample、MIDI tick を一つの transport と playhead で同期する。
  - realtime playback と offline export が同じ routing/automation result を出す。
  - overload 時は drop/glitch/xrun を計測表示し、UI を固めない。
  - 実装開始時に audio backend の初期対応範囲を ADR で固定する。Windows は WASAPI shared/exclusive を基準とし、ASIO は SDK/license/distribution 条件を確認して対応可否を明示する。CoreAudio、ALSA/JACK 等も初期対象、後続対象、非対象を曖昧にしない。
  - reference hardware/project ごとに round-trip latency、callback deadline miss/xrun、最大 Audio/MIDI Track 数、同時 VST3 instrument/effect 数、CPU/memory budget を release build で計測し、機能列挙だけで完了扱いにしない。
  - sample-accurate automation と plugin delay compensation は impulse/click fixture で sample offset を実測し、realtime/offline の一致と許容差を固定する。
  - MIDI 1.0 note/CC/pitch bend/aftertouch/clock を baseline とし、MIDI 2.0、MPE、poly pressure、SysEx を初期対象、後続対象、非対象のいずれかへ ADR で分類する。
  - [ ] Phase A: Audio device 選択、sample rate/buffer、hot plug、record arm、monitoring、metronome、count-in、loop/punch と録音 file の crash-safe commit を完成する。
  - [ ] Phase B: MIDI device hot plug、clock/MTC 同期、record/import、piano roll、velocity、quantize、loop と sample-offset playback を完成する。
  - [ ] Phase C: Track/Bus/Send/Return/Master、insert、automation、latency compensation、VST3 instrument/effect と realtime mix を完成する。
  - [ ] Phase D: take lane/comping、time stretch/pitch shift、freeze/bounce、stem/master offline export と realtime/offline parity を完成する。

## M5: core-level extensible plugin kernel

- [ ] **部分実装：現 ABI を capability-oriented kernel へ一般化する。** `ruvie-plugin-api` ABI v1、manifest/discovery、Property、Style、Decorator、Effector、CPU RGBA8 Effect/Loader は実装済みだが、Exporter、GPU、Audio、MIDI、Node、job/provider、panel は未対応。
  - Plugin Manifest は stable plugin/component ID、semantic version、ABI range、capabilities、permissions、platform binary、optional worker/UI resources を宣言する。
  - Host は capability negotiation を行い、未対応 capability を load 成功に見せない。
  - Project は plugin ID/version/state/provenance を保存し、Rust trait object、DLL pointer、GPU handle を保存しない。

- [ ] **部分実装：core capability extension interface を ABI/runtime へ実装する。** `docs/adr/0006-plugin-extension-kernel.md` で所有権、capability、transport、lifecycle、failure semantics を固定済み。Plugin が Importer/Decoder/Encoder/Exporter、Effect/Transition、Generator/DataSource、Analyzer、ASR/TTS、Audio Processor/Instrument、MIDI Processor、Module Node、GPU kernel/material、command/tool、panel schema、background job を追加できる typed contract を ABI と runtime に実装する。
  - plugin-defined Asset/Source/Module/Transition/Tool は stable component ID、typed descriptor、opaque authored state で登録する。DLL 固有型や任意 variant を Core enum/model へ注入しない。
  - Plugin は immutable Project snapshot と bounded Host Services を受け、直接 Project を変更せず、validated edit proposal/asset result を返す。
  - Core の transaction service が proposal を適用し、selection、validation、Undo/Redo、dirty state を一括管理する。
  - Timeline placement/hierarchy/time、Module published boundary、RenderPlan scheduling/cache dependency は Core の owner のままにする。
  - missing/disabled/version-mismatch plugin でも opaque authored state と provenance を保持し、silent substitute、state drop、Core 側 fallback を行わない。
  - hot path は versioned typed/batched ABI または shared buffer を使い、frame/audio/particle を一要素ずつ JSON で渡さない。

- [ ] **未実装：plugin process、trust、resource policy を実装する。** Native in-process plugin は trusted のみとし、外部モデル、cloud provider、VST3、AE compatibility host、未知 decoder は原則 worker process へ隔離する。
  - signature/trust prompt、permission（file/network/GPU/audio device/model download）、memory/time/output limits、cancellation/progress、crash recovery、structured diagnostics を提供する。
  - background job は project close/Undo/redo/retry に耐え、古い result を勝手に Timeline へ挿入しない。
  - pre-v1 では exact component schema を要求する。plugin 更新、missing plugin、schema mismatch は authored state を保持して診断し、Core 側の state upgrader、project migrator、fallback、dual-write を追加しない。

- [ ] **未実装：plugin SDK と conformance suite を提供する。** Rust/C header、fixture host、sample plugin、ABI fuzz/property test、malformed buffer、panic/crash/timeout、color/audio format、determinism、offline/realtime parity を検査する。plugin から追加した UI/action も native HTTP QA metadata を登録できるが、QA bridge の任意操作権は与えない。

- [ ] **未実装：plugin package lifecycle を完成する。** package の install、署名/trust確認、dependency/ABI resolution、enable/disable、更新、rollback、uninstall、cache cleanup を一つの Plugin Manager UI と service で扱う。Project が必要とする missing/version-mismatch component は authored state を保持して診断し、silent substitute や Core 側の互換分岐を作らない。

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

- [ ] **未実装：動画生成 provider plugin contract と最初の adapter を実装する。** prompt/reference image/video/seed/model/settings、credential、見積り/同意、async submit/poll/cancel/retry、result download、provenance を共通 provider capability 上に実装する。ユーザーが意図した「minmax」が MiniMax であることは provider 選定時に確認し、固有 API は adapter に隔離する。結果は Asset として受け取り、Core がユーザー確認後に Timeline へ配置する。provider API を Core や Project model へ直接埋め込まない。

- [ ] **未実装：再利用 Template system を完成する。** title/caption/character/motion/logo/effect/transition/particle/mixer/routing/workspace template を分類し、instance-local text/color/position が sibling を変えないようにする。shared Definition 編集は `この変更は N 個の Instance に反映` を明示し、copy-on-write を既定にする。

- [ ] **未実装：DataSource/Infographics plugin flow を実装する。** CSV/JSON/Table 等を stable row key で Generator に渡し、GeneratedItem と manual Override を作る。reload、row delete/split/key change の Active/Orphaned/Conflict UI と、position/color/text の一件だけの手修正保持を native QA する。

## M7: 統合 3D SceneRuntime、Camera、FBX、Particle、Plexus

- [ ] **部分実装：一つの GPU SceneRuntime に統合する。** `docs/adr/0003-stateful-gpu-scene-runtime.md` と OpenGL 4.3 compute/SSBO の Particle slice は存在する。別の 3D renderer/device を増やさず、既存 `SceneRuntime` を Model/Particle/Plexus 共通 scene pass owner へ一般化する。
  - Skia は 2D/text/vector/effect/composite を継続し、3D pass は color/depth/object-id target を生成して同じ Composition に合成する。
  - GL/Ganesh state、barrier、resource lifetime、device loss を一境界で管理し、Preview と Export が同じ RenderPlan command を使う。
  - scene source が複数でも Composition ごとの camera/depth 関係を保ち、Clip ごとに独立 flatten しない。

- [ ] **部分実装：GPU Particle の最初の executable slice を製品機能にする。** Emitter → Emitter Shape → Birth Attributes → Gravity → Drag → Sprite Renderer → Output の bounded Node Clip、fixed 1/120 s、seed、checkpoint、最大 capacity、per-instance state、shared compiled pipeline までは存在する。通常 Clip や Timeline は Node 化しない。
  - [x] Particle Node Clip factory、7 nodes/6 connections/16 Published Parameters、階層 RenderPlan の `ParticleSceneFrame`、既存 `SceneRuntime` への Preview 実行経路を実装した。
  - [x] 新規 Particle の出生位置を同じ canonical graph/runtime に統合した。curated Inspector と Node Editor は `Emitter Shape` の Point/Box/Sphere、Position、Radius/Size、Surface Only を同じ Published Parameter から編集し、`Initialize Particle` の表示名は役割を明確にする `Birth Attributes` とした。別の Inspector 専用 Particle model は追加していない。
  - [x] 実 GPU の可視出力を修正した。point sprite ではなく full affine に追従する 6-vertex quad を描画し、premultiplied alpha、非等方 scale/shear、透明重なり、singular transform、遠距離 seek/rewind、複数 GPU context の所有権を opt-in 実 GPU test で検証した。
  - [x] Preview と PNG/Video Export の frame 生成に同じ Particle scene semantics を通した。通常 Export は CPU renderer のまま維持し、選択 Output が Particle に到達するときだけ独立 GPU session を作る。GPU 不可時は audio temp、exporter、出力作成より前に明示診断し、partial file を残さない。Preview/PNG の最終 RGBA8 pixel parity は実 GPU test で検証済みだが、encoded Video の再 decode による画・timing・audio parity は M8 に残す。
  - [x] Assets の Generators セクションから初心者向けの `Particle System` として Timeline へ drag/drop し、一つの private Definition、Instance、5 秒 Item を一 transaction、一 Undo で作成する。通常 UI に Node/GPU/private の実装用語や上部の一時ボタンを出さない。
  - [x] curated Inspector は native descriptor の hard range、step、単位、automation capability を表示する。fixed-step sampling 未実装の simulation parameter に keyframe/expression/Node connection を作らせず、Sprite Color など実行可能な frame-local parameter だけを許可する。UI、authoring service、Project validation は同じ catalog contract を使う。
  - [ ] Emitter、Particle、Force、Renderer の一般設定は curated Inspector で扱い、`Edit Logic` ではその同じ canonical graph を production Node Editor で開く。Inspector 専用 Particle model と Node 専用 model を作らず、往復変換や二重同期を発生させない。
  - [x] production Node Editor の作成メニューには runtime status が Implemented の6種類の Particle Node だけを出す。DesignNeeded placeholder は隠し、定数専用 socket は理由を表示して無効化する。Sprite Renderer は通常の Image source として Effect、Merge、共通 Output terminal の前に接続できる。
  - [x] Node の状態意味論を固定した。disabled stage と bypass 不可能な Emitter/Sprite は no output、Emitter Shape/Birth Attributes/Gravity/Drag の bypass は中立値の type-preserving pass-through とする。実装済み modifier は型付きの正順で省略可能にし、編集中の不完全・未対応・順序違反 chain は Project 全体の compile error ではなく安定した no-image にする。
  - [x] Output reachability を capability、media dependency、runtime で共通化した。到達不能な binding は GPU capability、cache dependency、runtime 評価へ入れず、Transition の instance override も到達不能な必須 input を評価しない。Sprite branch ごとに state slot を分離しつつ、同じ Emitter ID から同一の deterministic random sequence を導出する。
  - [ ] simulation parameter の Timeline keyframe、expression、Published input を step boundary で deterministic に sample する。Preview、seek、reverse、repeat、Export が同じ schedule を使う。
  - [ ] Mesh emitter、velocity/lifetime/size/color over life、Vortex/Field/Turbulence、collision、sprite texture/per-particle color、mesh/ribbon renderer を同じ typed `ParticleSystem` graph と runtime に追加する。Point/Box/Sphere emitter は完了済み。次は catalog 表示だけでなく fixed-step GPU kernel、RenderPlan parameter、Inspector、Preview/Export parity test を一つずつ同時に閉じる。
  - [ ] graph の編集単位を Spawn / Birth / Update / Render の stage として明示する。Unity VFX Graph の Context、Unreal Niagara の Emitter/Particle Spawn・Update stack と同様に「いつ一回だけ評価され、いつ毎 step 評価されるか」を header/help/接続規則で見せる。ただし既存の一つの typed `ParticleSystem` graph を別 model へ置換しない（参考: https://docs.unity.cn/Packages/com.unity.visualeffectgraph%4016.0/manual/GraphLogicAndPhilosophy.html, https://dev.epicgames.com/documentation/en-us/unreal-engine/overview-of-niagara-effects-for-unreal-engine）。
  - [ ] 他の Particle、Model、Image、Audio、Field を参照する機能は、内部 UUID を文字列で指すのではなく typed Published input/Data Interface として実装する。Mesh emitter、collision、sprite texture、event spawn、Plexus point source が同じ Module invocation dependency と InstancePath を使い、Houdini POP の context geometry のように明示 input から source geometry を受け取る（参考: https://www.sidefx.com/docs/houdini/nodes/dop/popsource）。
  - [ ] Emitter/force/renderer ごとに world/local/parent space、XYZ orientation、angular velocity、camera-facing/axis-facing、depth sort policy を型として定義し、3D transform、Camera、Model と同じ scene coordinates を使う。
  - [ ] real GPU の seek/reverse/repeat/export parity、OOM/device unsupported diagnostics を必須 suite として通す。ローカル実 GPUでは非透明描画、deterministic seek/rewind、独立 renderer、context teardown、Preview/PNG parity、解像度に依存しない logical-space projection が通過済み。残りは GPU runner での必須化、OOM/device-loss matrix、長時間 repeat である。複数 Particle layer の target 再利用と 60 fps 計測は M8 の継続 gate で扱う。

- [ ] **未実装：Plexus-style proximity geometry を Particle/point-cloud の上に実装する。** 別 simulator を作らず、GPU spatial hash/grid、neighbor search、max distance/max neighbors、stable edge identity と Point/Line/Triangle renderer node を追加する。
  - animated particle/model vertices を入力でき、distance/color/width/opacity を Published Parameter と Timeline keyframe で制御できる。
  - O(n^2) 全探索を避け、capacity と generated edge/triangle 数に hard limit、diagnostic、cache key を持つ。
  - deterministic seek と Preview/Export parity を golden/native GPU QA する。

- [ ] **未実装：Timeline 3D transform と Camera Item を実装する。** `docs/adr/0004-timeline-3d-space-and-camera.md` は契約のみで、利用可能機能ではない。
  - 2D/3D 共通 transform に position XYZ、anchor XYZ、scale XYZ、rotation XYZ、documented rotation order、parent matrix を持たせる。
  - Camera は interval/layer、perspective/orthographic、FOV/focal length、near/far/focus を持つ第一級 Timeline source とし、camera cut を通常の edit と keyframe で扱う。
  - Camera interval が重なる場合の active-camera 優先順位、Camera がない区間の fallback、cut 境界の frame/sample 丸めを deterministic な Timeline 規則として固定する。
  - Preview gizmo、bounds、picking は object-id/depth/evaluated geometry に一致し、Timeline/Dope Sheet/Curve Editor/Inspector が同じ property ID を編集する。
  - Nested Timeline は既定で flatten、明示時だけ 3D space を expose/collapse し、外側移動で内部 local animation を壊さない。

- [ ] **未実装：3D Material、Light、shadow を SceneRuntime に追加する。** scene-neutral Material と texture slot、unlit/PBR baseline、directional/point/spot/ambient Light、shadow caster/receiver、depth/normal/object-id pass を型付き RenderPlan command にする。FBX、Particle mesh、将来の glTF が同じ Material/Light を使い、Skia 2D compositing と color-management 境界を明示する。

- [ ] **部分実装：FBX/model import を統合 3D pipeline へ接続する。** FBX resource parser の基礎コードはあるが、Timeline 上で描画できる 3D asset pipeline は未完成。
  - mesh、hierarchy、transform、material/texture、camera、light、skeleton/skinning、blend shape/morph、animation curve interpolation、multiple takes、unit/axis conversion を import report 付きで扱う。各機能を supported/partial/unsupported に分類し、欠落を黙って静止 mesh にしない。
  - Asset preview、drag-to-Timeline、Model Node Clip input、Inspector、missing texture relink を実装する。
  - malformed/untrusted file limits、stable asset fingerprint、decode/cache、object picking、color management を検証する。
  - FBX 固有 decode と scene-neutral model representation を分離し、将来 glTF 等も同じ renderer を使えるようにする。

- [ ] **未実装：Motion Logo 向け 2D/3D motion primitive を揃える。** Shape/Text、SVG path edit、Mask/Matte、parent、anchor、constraint、path animation、Text Animator、curve/easing、Nested Timeline と `Fixed/Scale/Loop/Responsive` duration policy を Timeline から編集できる。Node は再利用ロジックの追加に限る。

## M8: 60 fps、cache、export、publish、QA

- [ ] **部分実装：realtime frame budget を守る。** VJ 用 reference scene/hardware を M0 baseline で固定し、release build の定常 playback で 60 fps、p95 frame work 16.67 ms 以下、10 分間の dropped frame 1% 未満、UI main-thread stall 50 ms 未満を目標/CI perf threshold にする。
  - 閾値を確定する前に、M0 の復旧 tag と clean main を同一 machine、同一 fixture、同一 release profile で計測し、GPU/driver/process memory を含む比較記録を残す。
  - render/decode/audio/plugin/model job を UI thread から分離し、最新 frame 優先の bounded queue と cancellation/backpressure を使う。
  - blur、thumbnail、waveform、video decode、Node zoom、dialog、plugin scan が無制限 work/memory を発生させない。
  - 複数 Particle layer で full-frame transient target を毎回作らず、同一 GPU owner の bounded pool、再利用、batching または直接 target 合成を実装し、reference scene の Preview/VJ 60 fps と memory budget を計測する。
  - preview resolution/quality degradation はユーザーに表示し、Export の意味論を変えない。

- [ ] **部分実装：cache と incremental scheduling を全 media に統一する。** RenderPlan、frame、decoded media、waveform、Module executable、scene pipeline/state/checkpoint、ASR/TTS/generated asset を共通 dependency/fingerprint policy で invalidation する。cache owner を二重化せず、budget、LRU、metrics、diagnostic を一箇所で追跡する。

- [ ] **部分実装：Preview/Audio/Export parity を保証する。** Project linear RGBAF32 と encoded sRGBA8/plugin boundary、alpha、HDR/SDR、sample rate/channel layout、frame/sample rounding を明文化し、mosaic/diagonal_clip のような format mismatch を compile 時に診断/convert する。Preview と Export は同じ derived plan/effect/audio/scene semantics を使う。
  - [x] Particle を使用する Export は、選択 Output の到達性と実 frame range を先に走査し、到達する全 target 寸法について SceneRuntime allocation、実 shader/SSBO/FBO draw、同寸法の strict GPU Ganesh surface、texture ingestion を audio temp、exporter、出力作成前に検証する。GPU 非対応や寸法別 allocation failure では出力を開始しない。
  - [ ] Particle を含む encoded Video を再 decode し、複数 frame の画素、frame timing、Audio mux を Preview/RenderPlan の基準値と許容差内で比較する。
  - [ ] Project-linear GPU surface が RGBAF32 非対応時に使う RGBAF16 fallback について、負値・1.0 超の extended range、half-float 上限、Preview/Export 許容差を実 GPU test と仕様に固定する。

- [ ] **E1 部分実装：一般動画 Export を原子的に公開する。** 依存: 現行 authoring RenderServer/Exporter 経路、output-path identity 検査。destination と同じ filesystem の sibling staging file へ書き、renderer/effect/全 frame write/encoder finish/Audio temporary cleanup を含む job 全体の成功後だけ close/sync と atomic replace を行う。GPU preflight の早期診断だけで完了扱いにしない。
  - final destination は user-facing result、source alias 検査、logical job identity、同時実行排他に使い、Exporter が書く staging path と型で区別する。staging 名の違いで同じ destination への並行 job を許可しない。
  - host/Core が publication を所有する。Exporter の `finish` は encoder を閉じて待つだけで、先に render/write が失敗した job を独断で publish しない。
  - staging は destination と同じ parent に `create_new` し、terminal extension を保持する。成功時は `sync_all`、reserved regular-file identity と non-empty 検査、source alias と final destination の relative/absolute/symlink/hardlink alias 再検査、stage identity 再検査、handle close、atomic replace の順に行う。明示的な Windows 出力は UNC/VerbatimUNC を許可するが、自動 media locator の許可範囲は広げない。
  - Project 保存に既存する UUID sibling temporary file と Windows `MoveFileExW(REPLACE_EXISTING | WRITE_THROUGH)` を共有 utility へ抽出し、同じ staging/sync/replace を二重実装しない。
  - [x] destination が未存在の場合に正常作成し、既存 sentinel がある場合は成功時だけ置換する production worker test を通す。
  - [ ] renderer、effect、N frame 目の write、encoder finish、Audio cleanup、出力未生成、sync/replace の各 failure を注入し、既存 destination が byte-for-byte 不変、sibling staging が 0 件であることを検証する。Exporter session を開始した場合だけ `finish` は正確に一回、preflight 失敗では 0 回とする。
    - [x] N frame 目の write、encoder finish、出力未生成、reserved staging entry 差替え、destination entry 差替え、通常の replace failure は既存 destination 保持と staging cleanup を検証した。
    - [x] **worker panic を terminal failure に封じ込めた。** job 本体、同じ pinned Exporter の `finish`、worker request 外縁を別々にguardし、開始済み sessionの `finish` を正確に一回試行してからAudio/stagingをcleanupし、panic時だけrendererを破棄する。frame途中と`finish`のpanicについて、同じworker・同じlogical pathで次jobが成功し、各requestのcompletionが一件だけ、`published = false`、sentinel不変、staging/Audio 0件であるproduction worker testを追加した。unwind不能なabort/OOM/process killはこの契約に含めない。
    - [x] **一つのExport jobでExporter endpointを固定した。** registry callback中の同ID再登録は許可したまま、active jobの全frameと`finish`を開始時にsnapshotした同じ`Arc<dyn ExportPlugin>`へ送り、replacementは次jobからだけ使う。FFmpeg session lockはcallback panic後にcleanup可能な形で回復し、`finish`実行前にregistry guardを解放する。
    - [x] **実 Attachment Effect / Asset Loader 境界後の失敗を検証した。** 非ゼロ sigma の built-in Blur Attachment と、`TimelineEditorService::import_file` で取り込んだ実 `h264_24.mp4` の FFmpeg Video Loader を production RenderServer 経路で実行し、各 callback 成功直後の instance-scoped `#[cfg(test)]` one-shot seam から frame 0/1 の通常 error を注入した。frame 0 は Exporter 未試行のため `finish = 0`、frame 1 は先行 frame を受理済みのため pinned Exporter の `finish = 1` とし、既存 destination の byte-for-byte 保持、staging 0、`published = false`、失敗 completion 一件、同じ RenderServer での次 request 成功を検証した。Audio route を持つ frame 1 の Blur/Loader error では temporary Audio cleanup も確認した。さらに実 FFmpeg Loader 成功後の frame 1 panic で、Audio/staging cleanup、single failure completion、renderer 破棄後の同一 worker の復旧を確認した。
    - [x] **Audio temporary cleanup を結果へ合成した。** temporary Audio は生成直後から video Export coordinator が所有し、準備途中、Exporter 未試行の実 Blur frame 0 error、Exporter frame 0 error、通常完了、panic の全てが一つの明示 cleanup path へ戻る。`Interrupted` / `PermissionDenied` / `WouldBlock` は publication 前に bounded retry し、失敗が続けば publication を拒否して主失敗と cleanup 失敗を型付き `OperationAndCleanup` terminal result に保持する。RenderServer instance-scoped `#[cfg(test)]` seam により、一回だけの transient failure、明示 retry 全失敗、Audio 準備失敗との同時発生、Exporter failure との同時発生を検証し、completion 一件、sentinel 不変、staging 0、生成した正確な temporary path の消滅、同じ worker/project/plan/path での次 request 成功を確認した。Exporter/Loader panic でも explicit cleanup 1回、Drop cleanup 0回を直接観測した。Drop は明示 cleanup 後の最終 bounded fallback だけであり、恒久的な OS 障害下で必ず削除できるとは宣言しない。
    - [x] **`sync_all` と Windows sharing violation を owner 境界で検証した。** Project 保存と Video Export が共有する `AtomicFileTransaction::sync_staging` だけに、RenderServer instance ごとの `#[cfg(test)]` one-shot seam を置いた。同期失敗は全 2 frame と pinned Exporter の `finish` 成功後に発生し、`frames_exported = 2` と `published = false` を区別したまま既存 sentinel を byte-for-byte 保持し、staging 0、completion 一件、同じ worker/project/plan/path の次 request 成功を確認した。Windows では destination を `FILE_SHARE_READ | FILE_SHARE_WRITE`、すなわち delete-share なしで実際に保持し、identity 検査を通過した後の production `MoveFileExW(REPLACE_EXISTING | WRITE_THROUGH)` が `ERROR_ACCESS_DENIED` または `ERROR_SHARING_VIOLATION` で拒否されることを atomic utility と RenderServer の両方で検証した。handle 解放後の同一 path 再試行は正常に publish される。これは owner 境界の atomic visibility を検証するが、parent-directory fsync や電源断 durability の完了宣言ではない。
    - [x] **export cancellation を request contract にした。** request ID ごとの一つの token を開始前、Particle preflight、Audio window、frame 評価前、render/write 間、finish/Audio cleanup 後に確認する。Exporter 未試行なら `finish = 0`、試行済みなら `finish = 1`。production worker の7つの停止位置で typed `ExportCancelled`、`published = false`、destination 不変、staging/Audio 0、completion 一件、同じ ID/worker/path の再試行成功を確認した。finish または Audio cleanup の同時失敗は `OperationAndCleanup` に両方の原因を保持する。公開直前の atomic state transition によって、受理した cancel が publish される競合を防ぎ、公開開始後は cancel を拒否して terminal result を待つ。PNG も request registry を共有し、直接書込み前の cancel と書込み開始後の拒否、queued PNG の cancel が active Video に影響しないことを確認した。PNG に Video の atomic publication 保証を追加したわけではない。
    - [x] **Quit/New/Open を cancellation と cleanup に統合した。** 実行中 Export を追跡したまま cancel し、terminal cleanup 完了前に window close、service 差替え、次 export を許可しない。`RenderServer::Drop` は shutdown cancellation と export worker の同期 join を行う。長時間 Export 中の Quit/New/Open を native HTTP QA し、request ID を持つ cancel→terminal→action の順序、既存 destination 不変、staging/Audio cleanup を確認した。plugin callback 自体が停止しない場合の強制割込みや終了時間上限を保証したものではない。
  - [x] `frames_exported` と `published` を別の結果状態として定義し、commit failure を成功表示しない。
  - [ ] 実 FFmpeg 成果物を再 decode/`ffprobe` し、frame/audio metadata、duration、timing と Preview/RenderPlan 基準を検証する。
    - [x] production Ctrl+E から 360 frame/12秒の H.264 640×360 BT.709 と、一つの AAC 48 kHz stereo stream を生成し、video/audio/container の start 0・duration 12秒、先頭/中間映像の非黒、先頭1秒音声の非無音を実 decode で検証した。
    - [ ] decoded video/audio を同じ frame/sample range の Preview/RenderPlan 基準値と数値比較する。
  - [x] `RUVIE_QA_EXPORT_PATH` を known fixture 限定で解決し、production の File > Export/shortcut → worker → status 更新を native HTTP QA する。File dialog を迂回する別 Export endpoint を作らない。full QA は release app、smoke は debug app を使用する。
  - E1 の現契約は atomic visibility と、報告された失敗時に既存 destination を保持することまでとする。最終 identity 検査から path-based replace までの間に別 process が destination を差し替える競合を conditional replace で防ぐ契約ではない。電源断 durability も未完了であり、より強い保証を宣言する前に OS/filesystem ごとの conditional replacement、Unix の rename 後 parent directory `fsync`、Windows の write-through/共有違反、実 filesystem ごとの crash test を追加する。
  - Non-goals: resumable export、複数成果物の一括 transaction、process kill 後の aged orphan cleanup。production app から未参照だった公開旧 `ExportService` は M0 cleanup で削除済みであり、互換実装は追加しない。

- [ ] **部分実装：native HTTP QA suite を完走する。** 各 UI 変更で対象 interaction を loopback bridge から操作し、visible pixels、project state、selection、Undo/Redo、audio counters、QA metadata、error log を検証する。
  - [x] 2026-09-04、`python scripts/qa-runner.py --mode full --jobs 1` で 21 suite（Assets、Timeline、Preview、Path、Inspector、Effect、Dope Sheet、Curve、Node、Node Clip、Particle System、Audio、Ensemble、Transition、Color Palette、Settings、Unsaved を含む）が全件通過した。
  - [x] 2026-09-04、commit `adceb8c`（evidence `target/qa-runs/20260905T075938Z-full-44800`）の同じ release production app を使う 22 suite が全件通過した。追加した `video-export` は Ctrl+E、worker/status、H.264/AAC artifact、staging cleanup を実検証し、Color Palette、Node Clip conversion、終了を伴う Unsaved dialog 操作の高速実行時同期も回帰なく通過した。
  - [x] 2026-09-04、Export worker の panic 封じ込めと Exporter endpoint 固定を含む release production app で 22 suite を再実行し、全件通過した（evidence `target/qa-runs/20260905T085236Z-full-70992`）。`video-export` は production の Ctrl+E 経路から H.264/AAC の公開、status 更新、staging cleanup まで 18.078 秒で完了した。
  - [x] 2026-09-05、実 Attachment Effect / Asset Loader 成功後の export failure 回帰検証を追加した状態で release production app の 22 suite を再実行し、全件通過した（evidence `target/qa-runs/20260905T092204Z-full-41820`）。`video-export` は Ctrl+E から H.264/AAC の公開と cleanup まで 17.656 秒で完了した。
  - [x] 2026-09-05、Audio temporary cleanup の明示 owner/retry/型付き result 合成と Export failure status を追加した release production app で 22 suite を再実行し、全件通過した（evidence `target/qa-runs/20260905T101617Z-full-57532`）。`video-export` は一回目の Ctrl+E で 360 frame の H.264/AAC を公開し、二回目は directory destination を置換・削除せず `Export failed for …` と具体的 worker errorで終了した。harness が保存済み QA 成果物を戻した後、三回目は前回の Export-owned error だけを消して `Exporting …` へ遷移し、同じproduction command/worker/pathで再公開した。全体35.969秒、staging 0、ERROR log 0を HTTP state と filesystem から確認した。
  - [x] 2026-09-05、共有 atomic owner の `sync_all` one-shot failure と実 Windows no-delete-share `MoveFileExW` failure/retry を追加した release production app で 22 suite を再実行し、全件通過した（evidence `target/qa-runs/20260905T104444Z-full-78300`）。`video-export` は production Ctrl+E の正常公開、directory destination を保持した terminal failure、同じ UI/worker/path での再公開を 34.391 秒で完了し、360 frame H.264、AAC 48 kHz stereo、staging 0、全 suite の ERROR/panic log 0を確認した。
  - [ ] `qa-particle-node-clip-e2e.py` が Assets からの drag、Timeline placement、16 Published Parameter の Inspector、Seed と Emitter Shape/Position の Instance override、同一フレーム Preview pixel 差分、Undo/Redo の完全復元、実 GPU Preview の時間変化/seek再現性、production Node Editor の有限 catalog と無効 socket を native HTTP 実操作で検証する。11 parameter 版の native QA は通過済みだが、Emitter Shape 追加後の統合 release binary で再実行するまで未完了扱いとする。
  - [x] `qa-particle-persistence-e2e.py` で、Assets dragから作成した Particle System の Instance override を production Save で project file へ保存し、native app を終了した。別プロセスの production `TimelineEditorService::open` で再読込みし、Item/Definition/Instance、override、同一 frame の Preview pixel hash、非透明 pixel 数が一致し、再読込み前後で project file hash が変わらないことを検証した。
  - [ ] 上記 UI suite と別に、対応 GPU を持つ自動 runner で opt-in 実 GPU test を ignored のままにせず、非透明 pixel、deterministic seek、独立 renderer、正常 teardown、Preview/Export parity を必須検査にする。
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
