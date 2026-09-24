# Writing a view

A view is a wasm32 cdylib on `view-guest`. It builds a widget tree the host
lays out and draws, hears meaning-level events back, and asks the host for
data through a fixed table of doors. This page is the whole surface.

## What is gpui and what is ours

The rule: we re-implement only what touches the host or the wire; everything
else is the gpui fork `Cargo.toml` pins (`gpui-pre`, rev `5545e01`),
re-exported unchanged. `src/lib.rs:5-13` is the gpui list (`px`, `rems`,
`Hsla`, `StyleRefinement`, `Styled`, `ElementId`, `SharedString`, the
`*Event` types, `Role`, ...). Ours, defined in this crate:

- `App`, `AsyncApp`, `Context`, `Entity`, `WeakEntity` (`src/context.rs`),
  `Task` (`src/executor.rs`), `Window` (`src/window.rs`): the entity graph
  and the tick loop run inside the guest, so the host never sees a closure.
- `div`, `uniform_list`, `list`, `Input`, `Editor`, `img`/`Img`, `svg`/`Svg`,
  `canvas`/`Canvas`, `surface`, `InteractiveText`/`StyledText`, `sensor`,
  `resize_handle`, `modal_overlay` (`src/element.rs`, `src/list.rs`,
  `src/editor.rs`, `src/primitives/`, `src/rich_text.rs`, `src/behavior.rs`):
  each lowers to a `view_wire::Node`, with handlers kept guest-side and
  crossed as per-frame indices.
- `InteractiveElement`, `StatefulInteractiveElement`, `FocusHandle`
  (`src/interactivity.rs`): listeners and focus become wire routes.

## The doors

`view-wire/src/doors.rs` is the one list of what a view may ask for: 28
kinds, each a marker type naming its request and reply (borsh both ways;
`host.widget` alone is MessagePack, because it names tree ids). The trait is
sealed, so a view cannot invent a kind. Three verbs on `Host` (`src/host.rs`):

```rust
let reply = cx.host().ask::<Query<Identity>>(identity::Query::List { page }).await?;
let mut live = cx.host().subscribe::<RpcLive>(valset::PROGRAM.into());
cx.host().notify::<doors::HostBadge>(3);
```

A node program is addressed by a `doors::Program` impl beside the view
(`crates/app/forge-view/src/api.rs`), never by the program crate; `Query<P>`
and `Submit<P>` are its two doors. Every refusal is `abi::Refusal`
(`reason` token, `sentence`), one type end to end. `Loaded<T>` + `cx.load`
(`src/view.rs`) hold an ask's four states and snapshot `Loading` as `Idle`.

`Session` (`doors.rs`, `subscribe::<HostProps>`) is what every view is handed:
`connected`, `dark`, `chain`, `account`, `endpoint`; an item per change.

## Lifecycle, snapshot

`View` (`src/view.rs`): `new(window, cx)` on first mount, `restored` after a
snapshot came back, `PREFERRED_WINDOW_SIZE` as `"w,h"` or `"none"`. The
snapshot is the view's own serde (`src/snapshot.rs`), refused while work is
pending; a host holds it to `view_wire::MAX_SNAPSHOT_BYTES` (8 MiB,
`view-wire/src/snapshot.rs`). Derive `Serialize`/`Deserialize` and keep
`Task`s out of the state (`Loaded` does).

## Exporting

`export_view!(View, "Name", "description", ["rpc", "host"])` (`src/view.rs`)
writes the five wasm exports and the manifest section `ducktape.view.manifest`
(`view-wire/src/manifest.rs`: header, name, description, capabilities,
preferred size, `WIRE_EPOCH`). Each capability must be one of
`doors::CAPABILITIES`, the `<capability>` half of the kinds the view asks
through; another literal is a compile error.

The ABI gate (`view-wire/src/abi.rs`, `tools/check-view-abi.py`): exactly one
import, `ducktape_view.panicked`, and exactly five function exports,
`alloc`/`init`/`tick`/`snapshot`/`restore`. `make wasm-views` builds every
view in `VIEWS`, runs `wasm-opt`, checks the ABI and prints the size;
`make view-wasm-check`
proves nothing in `VIEW_LINKABLE` reaches `VIEW_FORBIDDEN`. Wire bytes are
pinned by `view-wire/tests/golden.rs`: a shape change bumps `WIRE_EPOCH`.

## Testing

`testing::TestAppContext` (`src/testing/context.rs`) opens a view over a
`FakeHost` (`src/testing/fake_host.rs`): `handle::<Door>`, `refuse`,
`stream`, `asked`; then `simulate_click`, `texts`, `assert_accessible`.
Screen export: a test gated on `*_SCREEN_EXPORT=1` (`FORGE_SCREEN_EXPORT`,
`crates/app/forge-view/src/screen_tests.rs`; `CHAT_SCREEN_EXPORT`,
`crates/app/chat-view/src/tests.rs`) writes each screen's tree as JSON under
`target/`; the app renders those with `dev/screens/chat-screens.sh
FIXTURES_DIR OUTPUT_DIR` (`ducktape-app --render-tree`, debug builds).
