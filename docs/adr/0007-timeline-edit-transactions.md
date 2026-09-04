# ADR 0007: Timeline edit transactions

- Status: Accepted; implementation is staged
- Date: 2026-09-05

## Context

RuViE は、一つの Track に複数の Clip を置ける階層型 Timeline を、通常編集の中心にする。

Select、Trim、Ripple、Insert、Overwrite などの操作が増えると、一回の操作が複数の Clip、Track、Marker、Transition に波及する。

drag 中の表示だけを UI が計算し、確定時の変更だけを service が別の規則で計算すると、snap、layer、ripple、依存オブジェクトの結果がずれる。

また、複数の service 呼び出しで一操作を実現すると、途中の validation 失敗、複数 Undo、部分的な変更が発生する。

[ADR 0001](0001-node-islands.md) により、配置、時間、Track、layer、親子関係、Transition は Timeline が所有する。

[ADR 0006](0006-plugin-extension-kernel.md) により、plugin は Project を直接変更せず、core に Timeline edit request を提出する。

本 ADR は、この二つの所有権を保ったまま、M3 のすべての Timeline tool が共有する編集トランザクションを定義する。

## 現状のコード監査

以下は 2026-09-05 時点の production authoring path の状態である。

| 項目 | 状態 | コード根拠 |
|---|---|---|
| Project mutation の owner | 実装済み | [`TimelineEditorService`](../../crates/library/src/editor/timeline_editor_service.rs#L1-L6) は唯一の mutable `AuthoringSession` を所有し、UI へ immutable snapshot を返す。 |
| 原子的な mutation と Undo | 実装済み | [`AuthoringSession::transact`](../../crates/library/src/model/authoring/edit.rs#L102-L128) は Project を clone した candidate に変更と全体 validation を行い、成功時だけ authoritative Project を交換して一つの Undo snapshot を積む。 |
| revision | 実装済み | [`ProjectRevision`](../../crates/library/src/model/authoring/edit.rs#L7-L25) と [`ChangeSet`](../../crates/library/src/model/authoring/edit.rs#L50-L54) は存在する。 |
| 単一 Clip の move/trim/split/duplicate/delete | 実装済み | [`timeline_editor_service/item.rs`](../../crates/library/src/editor/timeline_editor_service/item.rs#L3-L148) の各操作は個別に `transact` を呼ぶ。 |
| layer 並べ替えの query | 実装済み | [`ordered_track_item_ids` と `track_item_ids_after_placement`](../../crates/library/src/model/authoring/timeline.rs#L74-L107) は安定した順序を返し、commit と row preview の双方で利用される。 |
| drag の preview origin | 部分実装 | [`TimelineItemGesture`](../../crates/app/src/state/authoring.rs#L117-L144) は一つの Clip の original と projected fields を分ける。 |
| drag preview と commit | 部分実装 | [`update_item_projection`](../../crates/app/src/ui/panels/timeline/interaction.rs#L141-L213) が UI で位置を計算し、release 時に [`move_item` または `trim_item`](../../crates/app/src/ui/panels/timeline/interaction.rs#L354-L395) を別途呼ぶため、共有された core `EditPlan` ではない。 |
| selection container | 部分実装 | [`AuthoringSelectionState`](../../crates/app/src/state/authoring.rs#L59-L115) は複数選択と primary を表現でき、[`ui/selection.rs`](../../crates/app/src/ui/selection.rs#L8-L54) は共通 modifier 規則を持つ。 |
| Timeline の複数選択編集 | 未実装 | Timeline の click と drag start は [`selection.replace`](../../crates/app/src/ui/panels/timeline/mod.rs#L348-L374) を呼び、gesture は一つの `item_id` しか保持しない。 |
| snap | 部分実装 | [`snap_seconds`](../../crates/app/src/ui/panels/timeline/geometry.rs#L110-L141) は move 時の frame と Item edge を 7 screen px 以内で選ぶが、UI にあり、Trim は frame 丸めだけである。 |
| linked Audio/Video | 未実装 | [`TimelineItem`](../../crates/library/src/model/authoring/timeline.rs#L53-L72) と [`AuthoringProject`](../../crates/library/src/model/authoring/project.rs#L68-L84) に link relation はない。 |
| track lock と sync lock | 未実装 | [`TimelineTrack`](../../crates/library/src/model/authoring/timeline.rs#L35-L43) は ID、Timeline、name、kind、properties だけを持つ。 |
| target track | 未実装 | `AuthoringTimelineView` に target track state はなく、library drop は pointer row から destination を直接決める。 |
| ripple と overlap policy | 未実装 | production authoring path に edit mode を表す型はなく、`move_item` は指定 Item 以外の時間を変更せず、重なりを許容する。 |
| Timeline-owned Transition | 部分実装 | [`Transition`](../../crates/library/src/model/authoring/transition.rs#L16-L50)、[`validate_transitions`](../../crates/library/src/model/authoring/project/validation.rs#L319-L434)、[`CompiledTransition`](../../crates/library/src/core/render_plan/model.rs#L138-L172) は存在する。 |
| Transition の tool 追従 | 未実装 | 現在の Item edit は Transition を更新せず、candidate validation を満たせない move/trim/delete を全体として拒否する。 |
| Marker | 未実装 | `AuthoringProject` に Marker collection はなく、authoring model に Timeline/Clip Marker 型はない。 |

`AssetKind::Video` の一つの `SourceRef::Asset` は Image と Audio の双方を提供できるが、これは二つの配置を連動編集する linked AV relation ではない。

その判定は [`AuthoringProject::item_supports_output`](../../crates/library/src/model/authoring/project.rs#L686-L733) にある。

## Decision

すべての production Timeline tool は、core の一つの pure planner と一つの commit path を使う。

概念 API は次の形とする。

```rust
fn plan_timeline_edit(
    project: &AuthoringProject,
    request: &TimelineEditRequest,
) -> Result<EditPlan, EditPlanError>;

fn project_edit_plan(
    project: &AuthoringProject,
    plan: &EditPlan,
) -> Result<EditProjection, EditPlanError>;

TimelineEditorService::commit_edit_plan(
    plan: EditPlan,
) -> Result<ChangeSet, LibraryError>;
```

`plan_timeline_edit` と `project_edit_plan` は filesystem、decoder、plugin callback、GPU、clock、random generator、global mutable stateへアクセスしない。

`EditPlan` は Timeline、Node、RenderPlan とは別の永続モデルではなく、一回の gesture または command にだけ存在する host-owned derived data である。

`EditPlan` は project file に保存せず、plugin から任意の内容を注入させない。

## Authoritative types

実装時の責務境界は次の型に集約する。

```rust
TimelineEditRequest {
    base_revision: ProjectRevision,
    timeline_id: TimelineId,
    operation: TimelineEditOperation,
    selection: EditSelection,
    target: EditTarget,
    policies: EditPolicies,
    snap: SnapContext,
}

EditSelection {
    primary_item_id: TimelineItemId,
    selected_item_ids: BTreeSet<TimelineItemId>,
    linked_selection: LinkedSelectionPolicy,
}

EditPolicies {
    ripple_scope: RippleScope,
    overlap_policy: OverlapPolicy,
    transition_policy: TransitionFollowPolicy,
}

EditPlan {
    base_revision: ProjectRevision,
    timeline_id: TimelineId,
    operation: TimelineEditOperation,
    mutations: Vec<EditMutation>,
    affected: AffectedObjects,
    snap_result: Option<SnapResult>,
    invalidations: Vec<ProjectInvalidation>,
    summary: EditPlanSummary,
}
```

正確な field の配置は実装時に既存 module へ統合してよいが、`TimelineEditRequest`、`EditPlan`、planner、projection、commit の責務を複製してはならない。

`EditMutation` は再計算用の高水準 command ではなく、stable ID、expected before-state、final after-state を持つ host-only mutation である。

新しい Item、Marker、Transition が必要な操作では、stable ID を planning 時に一度だけ確保し、preview と commit で同じ ID を使う。

## Planning order

planner は次の順序を固定する。

1. `base_revision`、`timeline_id`、primary、selection の存在と Timeline 所属を検証する。
2. pointer drop または transient target state から、明示された destination Track と layer slot を解決する。
3. selection を linked group により展開し、各対象に選択理由を記録する。
4. direct、linked、ripple の全候補に hard track lock を適用する。
5. immutable origin から raw time delta、trim boundary、source mapping を exact `MediaTime` で計算する。
6. selection 全体の一つの anchor delta に snap を適用する。
7. destination に `OverlapPolicy` を適用する。
8. `RippleScope` と sync lock から後続 Item の移動集合を確定する。
9. Marker と Transition の追従、remap、resize、remove を確定する。
10. stable ID 順の final mutations と invalidation ranges を作る。
11. mutations を sparse overlay view に適用して affected invariant を検証し、成功した場合だけ `EditPlan` を返す。

pointer update ごとに Project 全体を clone して全件 validation する実装を完成形にしてはならない。

全体 validation は commit の candidate Project に対して一回行い、planner と commit の同値性は property-based test で保証する。

この順序は UI tool ごとに入れ替えない。

## Selection semantics

Selection は transient UI state であり、Project に永続化しない。

`EditSelection` は active Timeline 内の Item だけを含み、primary を必須とする。

foreign Timeline の Item、存在しない ID、selected set に含まれない primary は request error とする。

未選択の Clip を plain drag した場合は、その Clip だけで selection を置き換える。

選択済みの Clip を plain drag した場合は、selection を維持し、選択された全 Clip を同じ rigid time delta で移動する。

Shift、Command/Ctrl、Shift+Ctrl の意味は既存の `SelectionAction` を Timeline と Preview で共用する。

layer reorder の primary は pointer の Track/layer slot へ移り、同一 Track にある他の選択 Item は現在の相対 layer order を維持した連続 block として配置する。

複数選択の一部だけを暗黙に除外して成功扱いにしてはならない。

操作に対応しない selection が含まれる場合は、対象 ID と理由を列挙して全体を失敗させる。

## Linked Audio/Video

複数の Timeline Item を同期編集する link は、stable `LinkGroupId` を持つ Timeline-owned relation とする。

Asset path、source asset ID、Clip name、同じ start time から link を推測しない。

一つの Item は高々一つの link group に所属し、group member は同じ Timeline に属する。

一つの Video Item が Image と embedded Audio を同時に出力する現行表現は、そのまま一つの Item として扱う。

linked AV は、映像と音声を別 Item に分離した場合、別録り音声を明示的に同期した場合、または user が明示的に link した場合に使う。

`LinkedSelectionPolicy::FollowLinks` は既定値とし、selected Item の group member を edit cohort に追加する。

一時的な link bypass は request に `LinkedSelectionPolicy::IgnoreLinks` として明示し、Project の link 自体を切断しない。

Move は全 member に同じ Timeline delta を適用する。

Trim は対応する edge に同じ Timeline delta を適用し、全 member が有効な source range を持てない場合は全体を拒否する。

Split は全 member が cut time を含む場合に全 member を分割し、left group と right group の link をそれぞれ維持する。

Delete は展開された全 member を同じ plan で削除する。

Link と unlink 自体も `AuthoringSession::transact` を通る一回の Undoable edit とする。

## Track lock and sync lock

`TimelineTrack` は persisted Timeline editing policy として `edit_locked` と `sync_lock` を持つ。

`edit_locked` は hard lock であり、direct、linked、overwrite、ripple、Marker/Transition dependency のいずれであっても、その Track の Item または Track-owned state を変更させない。

hard-locked Track の mutation が一つでも必要なら、既定動作は全 plan の失敗である。

planner は hard-locked Track を黙って飛ばして同期を壊してはならない。

`sync_lock` は保護 lock ではなく、`RippleScope::SyncLockedTracks` が時間 delta を共有する Track を選ぶための opt-in である。

source または destination Track は direct edit の seed として常に scope に入り、他の `sync_lock` Track が追加される。

hard lock と sync lock の双方が有効な Track に mutation が必要な場合は、hard lock が優先され、plan は失敗する。

Track lock の切り替えは Project edit として Undoable にするが、現在の selection は変更しない。

## Target track

Target track は Project content ではなく、workspace の transient targeting state である。

pointer drop が具体的な Track/layer を指す場合は、その destination を request に明示する。

keyboard insert、paste、record、source patch など pointer destination がない操作は、media kind ごとの enabled target track を使う。

planner が受け取る `EditTarget` は解決済みの `TimelineTrackId` と layer/insertion slot を持つ。

target が missing、foreign Timeline、media-incompatible、hard-locked の場合は具体的な error を返す。

別の unlocked Track を自動探索して成功扱いにしてはならない。

linked AV の複数 stream を別 Track に置く操作では、各 stream と resolved target の対応を request にすべて含める。

## Snap

Snap は app の geometry helper ではなく、core planner が所有する pure policy とする。

`SnapContext` は enabled target kinds、exact tolerance、primary anchors、optional playhead/in-out points を持つ。

screen pixel tolerance は UI が現在の共通 viewport transform から一度だけ `MediaTime` に変換し、planner 内では pixel や `f64 seconds` を再参照しない。

Project から導出する候補は frame/grid、Item start/end、Marker start/end、Transition edit point/start/end である。

playhead と in/out は transient candidate として request に含める。

moving cohort 自身と、同じ plan で削除される候補は snap source から除外する。

複数選択では primary start/end または tool が指定した一つの boundary を snap anchor とし、結果の一つの delta を cohort 全体へ適用する。

各候補は `SnapTarget { kind, time, owner_id }` として識別し、preview guide と QA metadata が同じ target を参照する。

候補は距離、固定された kind priority、time、stable owner ID の順で比較し、HashMap iteration order に依存させない。

`EditPlan::snap_result` は選ばれた target、raw time、snapped time、applied delta を保持する。

commit は再 snap せず、plan に保存された exact result を適用する。

## Ripple scope

`RippleScope` は次の四種類に限定する。

```rust
enum RippleScope {
    None,
    TargetTrack,
    SyncLockedTracks,
    AllUnlockedTracks,
}
```

`None` は後続 Item を動かさない通常の move、trim、lift、overwrite に使う。

`TargetTrack` は direct destination Track だけの後続 Item を動かし、Timeline Marker は動かさない。

`SyncLockedTracks` は seed Track と `sync_lock` が有効な Track を動かし、`FollowRipple` の Timeline Marker に同じ Timeline time transform を適用する。

`AllUnlockedTracks` は hard-locked Track を除く active Timeline の全 Track を明示的に動かし、`FollowRipple` の Timeline Marker に同じ transform を適用する。

Ripple の対象は、request が持つ edit boundary と duration delta から導出する。

後続判定、境界上の Item、edit range を跨ぐ Item の split/trim 規則は operation type が決め、UI が独自に判定しない。

正の delta は insertion point 以後を送って gap を開く。

負の delta は extract range を時間変換し、range 後を詰める。

対象 Track の Item が edit range を跨ぐ場合、Extract は boundary で split/trim し、Ripple Trim は tool が指定した edge だけを trim する。

同じ Item を linked expansion と ripple expansion の双方が選んでも mutation は一つに正規化する。

各 affected object は `Direct`、`Selected`、`Linked`、`Ripple`、`TransitionDependency`、`MarkerDependency` の理由を plan summary に持つ。

## Overlap policy

重なりは暗黙の副作用ではなく、request の `OverlapPolicy` で決める。

```rust
enum OverlapPolicy {
    Stack,
    Overwrite,
    Insert,
    Reject,
}
```

`Stack` は指定 Track/layer に配置し、既存 Item の時間を変えない。

現在の layer 型 Timeline と intentional overlap は `Stack` を使う。

`Overwrite` は destination range と交差する対象 Track の Item を deterministic に trim、split、remove する。

`Insert` は配置 duration の gap を開き、request の ripple scope に従って後続を送る。

`Reject` は destination の media-conflicting interval が一つでもあれば、conflict IDs と ranges を返して変更しない。

hard lock、link、Transition dependency により必要な overwrite が実行できない場合は、全 plan を失敗させる。

Clip が重なっただけで Transition を生成してはならない。

Transition は preset drop、`Add Transition`、または明示 command でだけ作成する。

## Marker follow semantics

Marker は Timeline-owned first-class model として、少なくとも `Timeline` anchor と `ItemLocal` anchor を区別する。

```rust
enum MarkerAnchor {
    Timeline {
        timeline_id: TimelineId,
        range: TimelineInterval,
        ripple: TimelineMarkerRipplePolicy,
    },
    ItemLocal {
        item_id: TimelineItemId,
        range: TimelineInterval,
    },
}

enum TimelineMarkerRipplePolicy {
    FollowRipple,
    FixedTimelineTime,
}
```

`ItemLocal` Marker は Item の `TimeMap` によって表示位置を得るため、Item の move、rate、reverse、Nested Timeline placement に追従する。

Item trim で visible source range の外へ出た Marker は消去せず、Item に保持して非表示にする。

Item split では point Marker を visible local range を含む側へ remap し、boundary 上は right side を選ぶ。

split boundary を跨ぐ range Marker は left/right fragment に分け、right fragment の新 ID を planning 時に確保する。

Item delete はその Item に属する Marker を同じ plan で削除し、Undo で一緒に復元する。

`FollowRipple` の Timeline Marker は ripple operation の piecewise time transform を受ける。

Insert では boundary 以上の time を delta だけ送る。

Extract では range 前を維持し、range 内を開始 boundary へ畳み、range 後を削除 duration だけ詰める。

range Marker は start/end の双方へ同じ transform を適用し、結果が zero duration でも黙って削除しない。

`FixedTimelineTime` の Marker は ripple で動かさない。

将来の musical anchor は M4 の `TempoMap` ADR で追加するが、上記の absolute `MediaTime` 規則を暗黙に変更しない。

## Transition follow semantics

Transition は Item effect ではなく、二つの participant を持つ Timeline-owned relation のままにする。

planner は participant の mutation を検出し、Transition mutation を同じ `EditPlan` に含める。

両 participant が同じ rigid delta で同じ Track 上を移動する場合は、`edit_point` を同じ delta で移動し、duration、alignment、processor、parameters を維持する。

participant の trim または ripple 後も現在の interval が有効なら、Transition を変更しない。

現在の duration を維持できないが正の duration を確保できる場合は、alignment を維持した最大 duration へ縮める。

planner は from/to の可視範囲と compiled hidden handle requirement の双方を検証し、利用不能な source handle を具体的に診断する。

Split では Transition の必要範囲を一意に提供する segment へ participant ID を remap する。

participant を Delete または Extract する場合は、依存 Transition を同じ plan で削除する。

Move、Trim、Roll、Slip、Slide が participant relation を完全に切断し、正の有効 duration を作れない場合は、既定の `TransitionFollowPolicy::PreserveOrReject` で全 plan を失敗させる。

user が明示した `TransitionFollowPolicy::RemoveInvalid` の場合だけ、その Transition を plan summary と preview に表示した上で削除できる。

Transition の自動 resize/remove は preview projection に必ず現れ、commit 時に初めて判明する副作用にしない。

Transition 自体の duration handle drag、alignment change、processor change も通常の `TimelineEditRequest` と一つの Undo boundary を使う。

## Preview projection

gesture 開始時に immutable Project snapshot と `ProjectRevision` を固定する。

pointer update ごとに前回の projected value ではなく、固定した origin と現在の pointer intent から新しい request を作る。

planner は毎回その origin に対して `EditPlan` を作るため、丸め誤差や snap の累積による振動を起こさない。

`EditProjection` は `EditPlan` の final before/after states を参照し、Item rect、layer row、trim frame、gap、overlap、Marker、Transition、snap guide を描画する。

painting、hit testing、row insertion preview、QA metadata は同じ `EditProjection` を使う。

UI に第二の ripple、snap、overlap、Transition follow algorithm を置いてはならない。

preview のために authoritative Project、Undo stack、RenderPlan を変更しない。

preview render が完全な Project snapshot を必要とする場合は、`EditPlan` の共通 apply 実装から copy-on-write candidate を作る。

通常の Item rect、gap、Marker、Transition、snap guide の描画は sparse `EditProjection` だけを使い、pointer update ごとの Project 全 clone を要求しない。

## Commit and one-Undo boundary

pointer release、keyboard command completion、confirmed plugin proposal は、それぞれ一つの `EditPlan` を一回だけ commit する。

`commit_edit_plan` は既存の `TimelineEditorService` と `AuthoringSession::transact` を拡張して実装し、並行する transaction manager を追加しない。

commit は session write lock を取得した後、current revision と `plan.base_revision` の完全一致を最初に確認する。

revision が異なる場合は `StaleRevision { expected, actual }` を返し、自動 rebase、再 snap、部分適用を行わない。

UI は最新 snapshot から再 plan できるが、user が見た古い preview と異なる plan を同じ release event で黙って commit してはならない。

commit は全 `expected before-state` を検証し、同じ共通 apply を candidate Project に適用し、Project 全体を validate する。

成功時だけ candidate を authoritative Project と交換し、revision を一回進め、Undo entry と `ChangeSet` を一つずつ作る。

失敗時は Project、revision、Undo、Redo、selection、RenderPlan cache authority を変更しない。

no-op plan は Project を交換せず、revision と Undo を増やさない。

drag 中に何回 preview plan を作っても Undo entry は作らない。

multi-item、linked、ripple、overwrite、Marker、Transition の全 mutation は一つの Undo/Redo で戻り、Redo は planning や external work を再実行しない。

`ProjectInvalidation` は caller が推測せず、plan の before/after affected ranges から core が導出する。

## Failure atomicity and diagnostics

planner と commit は同じ typed `EditPlanError` taxonomy を使う。

最低限、`StaleRevision`、`MissingObject`、`ForeignTimeline`、`TrackLocked`、`IncompatibleTarget`、`InvalidSourceRange`、`OverlapConflict`、`LinkConflict`、`TransitionWouldDetach`、`UnavailableSourceHandle`、`ValidationFailed` を区別する。

error は primary cause、affected stable IDs、Timeline ranges、lock/link/ripple による選択理由、可能な user action を持つ。

一部の Track または Item だけを成功扱いにする warning fallback は設けない。

decoder、plugin、AI job、filesystem write など失敗可能な external work を `EditPlan` apply 中に実行しない。

external work は先に managed artifact へ stage し、その immutable result を参照する edit request だけを planner へ渡す。

## Ownership invariants

1. `EditPlan` は Timeline Item、Track、Marker、Transition、Timeline-owned automation だけを編集し、Module Definition の topology を変更しない。
2. Node Clip の move、trim、split、ripple は通常 Item と同じ planner を使い、Module graph を展開しない。
3. `SourceRef::Composition` の placement edit は外側 Timeline time だけを変え、内側 Timeline の local animation を書き換えない。
4. Transition は participant、timing、processor reference、parameters を Timeline が所有し、Node connection に変換しない。
5. Selection、target track、active tool、pointer、snap pixel tolerance は transient editor state である。
6. Link group、hard track lock、sync lock、Marker、Transition は Project の Timeline state である。
7. `EditPlan` と `EditProjection` は derived data であり、project file と RenderPlan に保存しない。
8. RenderPlan compilation は commit 後の validated Timeline snapshot だけを入力とする。

## Plugin edit requests

Plugin は `EditPlan` または `EditMutation` を作れない。

Plugin は versioned `TimelineEditRequest` の bounded subset と preconditions を提出する。

core planner は通常 UI と同じ selection、lock、link、target、snap、ripple、overlap、Marker、Transition 規則で dry-run する。

Host は `EditPlanSummary` を user に提示し、必要な confirmation 後に host-minted plan を一回 commit する。

stale または invalid な plugin request は何も変更せず、plugin に mutable Project、session、Undo handle を渡さない。

## Staged implementation

### Stage 1: pure single-item plan

- 現在の move と trim を core `TimelineEditRequest`、`EditPlan`、`EditProjection`、`commit_edit_plan` へ移す。
- 既存 `TimelineItemGesture` は plan を保持する transient gesture state に縮小する。
- 現在の `snap_seconds` と row placement calculation を planner に統合し、UI 側の重複計算を削除する。
- preview/commit parity、stale revision、no-op、one Undo を先に固定する。

### Stage 2: selection, links, locks, and overlap

- Timeline panel を既存共通 `SelectionAction` に接続し、selected cohort drag を実装する。
- Timeline-owned link group、`edit_locked`、`sync_lock` と transient target track を追加する。
- `Stack`、`Reject`、`Overwrite`、`Insert` を同じ planner に追加する。

### Stage 3: ripple and cut tools

- Ripple Trim、Ripple Delete、Insert、Extract を四つの `RippleScope` で実装する。
- Razor、Roll、Slip、Slide、Rate Stretch、Lift、Overwrite を新しい service path ではなく `TimelineEditOperation` として追加する。
- keyboard、context menu、drag handle は同じ request factory を使う。

### Stage 4: Marker and Transition follow

- Timeline/Item/range Marker を追加し、piecewise ripple transform を planner に実装する。
- 既存 Timeline-owned Transition を move/trim/split/delete/ripple/roll の dependency planning に接続する。
- hidden source handle diagnostics と Transition preview overlay を追加する。

旧 `move_item`、`trim_item` などの直接 mutation API は callsite 移行後に削除するか、同じ request factory を呼ぶ一時的な内部入口に限定する。

恒久的な第二 planner、UI-only snap、tool ごとの ripple helper を残してはならない。

## Acceptance tests

1. 同じ snapshot と pointer intent から作った preview projection と committed Project の全 affected before/after state が一致する。
2. drag 中に別 edit で revision が変わると commit は `StaleRevision` で失敗し、Project、Undo、Redo は変化しない。
3. 選択済みの三 Clip を drag すると三つだけが同じ delta で動き、未選択の上段 Clip は動かない。
4. linked Video/Audio の move、trim、split、delete は全 member を一 transaction で変更し、IgnoreLinks は link relation を壊さない。
5. direct、linked、sync ripple、overwrite のいずれかが hard-locked Track を必要とすると全体が失敗する。
6. missing または incompatible target track で別 Track へ fallback しない。
7. snap は zoom から変換した同じ tolerance と tie-break で deterministic になり、group の相対 offset を変えない。
8. 四つの ripple scope は exact expected Item set を返し、無関係な Track の Clip を動かさない。
9. `Stack` は intentional overlap を保ち、`Reject` は無変更、`Overwrite` は exact segments、`Insert` は exact scoped shift を作る。
10. 一回の multi-item ripple/overwrite は Undo 一回で完全に戻り、Redo 一回で同じ stable IDs と states を復元する。
11. Timeline Marker と ItemLocal Marker は定義した move、trim、split、insert、extract の time transform に従う。
12. Transition は両 participant の rigid move で追従し、trim で deterministic に preserve/resize され、participant delete で同じ plan から除去される。
13. Transition の不足 hidden handle は source ID、必要量、利用可能量を返し、Project を変更しない。
14. Node Clip を含む edit で Timeline placement だけが変わり、Module Definition hash と Node topology は変化しない。
15. plugin edit request と同じ request を UI から送った場合、同一の plan summary、validation、Undo result になる。
16. property-based test は、成功した全 plan について `project_edit_plan(origin, plan)` と `commit_edit_plan(plan)` の結果が一致し、結果 Project が validate されることを確認する。

## Consequences

Timeline tool の追加は、UI ごとの独自 mutation を増やす作業ではなく、一つの `TimelineEditOperation` と pure planner rule を追加する作業になる。

preview は確定結果の予測ではなく、確定に使う同じ plan の表示になる。

lock、link、ripple、Marker、Transition による波及は plan summary と diagnostics で説明できる。

繰り返し planning は immutable Timeline index と sparse projection を再利用し、10,000 Clip fixture の drag preview frame budget を性能テストで固定する。

index と projection の最適化でも `EditPlan` の意味論と共通 apply path は分岐させない。

通常 Clip 数が増えても Node 数は増えず、Timeline 編集の複雑さは Timeline-owned transaction の中に留まる。
