#!/bin/sh
# `make new-module NAME=x` / `make new-view NAME=x-view`: a module in
# chat's shape (types, rules and module always built, its wasm exports
# behind `module`, a native test over `guest::MockHost`) or a view in members-view's
# shape (links its module with `module` off, `export_view!`, one screen
# test), registered in the Makefile and the workspace. Run from the repo root.
#   tools/scaffold.sh module <name> | view <name>-view
set -eu
kind=$1
name=$2
dir=crates/app/$name
case "$name" in
    *[!a-z0-9-]* | -* | *-) echo "$name: a crate name is lowercase words joined by '-'" >&2; exit 1 ;;
esac
test ! -e "$dir" || { echo "$dir exists" >&2; exit 1; }
snake=$(echo "$name" | tr - _)

register() { # <Makefile list> <name>
    sed -i "s/^$1 := .*/& $2/" Makefile
    sed -i "s|^    \"crates/lib/gitcore\",|    \"crates/app/$2\",\n&|" Cargo.toml
}

module() {
    # The module type: TitleCase of the module name.
    title=$(echo "$name" | awk -F- '{ for (i = 1; i <= NF; i++) printf "%s%s", toupper(substr($i, 1, 1)), substr($i, 2) }')
    mkdir -p "$dir/src" "$dir/tests"
    cat > "$dir/Cargo.toml" <<EOF
[package]
name = "$name"
version.workspace = true
edition.workspace = true

# The types, rules and module are always built; the view links them with
# \`module\` off. \`module\` adds its wasm exports (\`guest::export!\`),
# which only its own wasm build turns on.
[lib]
crate-type = ["cdylib", "rlib"]

[features]
module = []

[dependencies]
abi = { workspace = true }
borsh = { workspace = true }
guest = { workspace = true }
store = { workspace = true }
EOF
    cat > "$dir/src/lib.rs" <<EOF
//! The \`$name\` module: one counter, to be replaced by what it keeps.
//!
//! Writes are an [\`Op\`] (borsh), reads a [\`Query\`] answered by a [\`Reply\`]
//! (borsh); \`$name-view\` links the same types. [\`$title\`] is the module:
//! one match over every op and one over every query. It runs natively over
//! \`guest::MockHost\`; the \`module\` feature adds its wasm exports, which a
//! view never enables.
use borsh::{BorshDeserialize, BorshSerialize};
use guest::{Error, ExecCtx, Module, QueryCtx};
use store::Item;

pub const MODULE: &str = "$name";

/// The one value this module keeps.
const COUNT: Item<u64> = Item::new("count");

#[derive(Clone, Debug, BorshSerialize, BorshDeserialize)]
pub enum Op {
    Bump { by: u64 },
}

#[derive(Clone, Debug, BorshSerialize, BorshDeserialize)]
pub enum Query {
    Count,
}

#[derive(Clone, Debug, BorshSerialize, BorshDeserialize)]
pub enum Reply {
    Count(u64),
}

pub struct $title;

impl Module for $title {
    type Op = Op;
    type Query = Query;
    type Response = Reply;

    fn execute(ctx: &ExecCtx, op: Op) -> Result<(), Error> {
        match op {
            Op::Bump { by } => bump(ctx, by),
        }
    }

    fn query(ctx: &QueryCtx, query: Query) -> Result<Reply, Error> {
        match query {
            Query::Count => Ok(Reply::Count(COUNT.get(ctx)?.unwrap_or_default())),
        }
    }
}

#[cfg(feature = "module")]
guest::export!($title);

fn bump(ctx: &ExecCtx, by: u64) -> Result<(), Error> {
    COUNT.update(ctx, |count| *count = count.saturating_add(by))?;
    Ok(())
}
EOF
    cat > "$dir/tests/$snake.rs" <<EOF
use guest::{Cause, Env, MockHost, Module, Origin};
use $snake::{$title, Op, Query, Reply};

fn env() -> Env {
    Env {
        chain_id: b"net".to_vec(),
        height: 7,
        time: 100,
        module: $snake::MODULE.into(),
        origin: Origin::Signed(vec![1]),
        cause: Cause::Direct,
    }
}

#[test]
fn bumps_add_up_and_read_back() {
    let host = MockHost::default();
    $title::execute(&host.exec(env()), Op::Bump { by: 2 }).unwrap();
    $title::execute(&host.exec(env()), Op::Bump { by: 3 }).unwrap();
    let Reply::Count(count) = $title::query(&host.query(env()), Query::Count).unwrap();
    assert_eq!(count, 5);
}
EOF
    register PROGRAMS "$name"
    sed -i "s|^forge = { path = \"crates/app/forge\" }|&\n$name = { path = \"$dir\" }|" Cargo.toml
    cat <<EOF
$dir/{Cargo.toml,src/lib.rs,tests/$snake.rs}, PROGRAMS, workspace members and dependencies.
Next:
  1. name \`$name\` in a founding (qa's founding.toml, or the module's params it seats with)
  2. write the contract: replace Op/Query/Reply and the module in src/lib.rs; \`make dev P=$name\`
  3. tell qa's kit about it (the pack step in kit's build, if it ships a view)
EOF
}

view() {
    case "$name" in *-view) ;; *) echo "$name: a view is named <module>-view" >&2; exit 1 ;; esac
    program=${name%-view}
    program_snake=$(echo "$program" | tr - _)
    test -d "crates/app/$program" || { echo "crates/app/$program is not there: make new-module NAME=$program first" >&2; exit 1; }
    upper=$(echo "$program_snake" | tr a-z A-Z)
    # The view type: TitleCase of the module name.
    title=$(echo "$program" | awk -F- '{ for (i = 1; i <= NF; i++) printf "%s%s", toupper(substr($i, 1, 1)), substr($i, 2) }')
    mkdir -p "$dir/src"
    cat > "$dir/Cargo.toml" <<EOF
[package]
name = "$name"
edition.workspace = true
version.workspace = true
publish = false

# A Rust-authored view, exported as a dynamically loaded wasm module by
# \`export_view!\`; \`unsafe_code\` is not forbidden only because the module
# exports need \`#[unsafe(export_name)]\`.
[lib]
crate-type = ["cdylib", "rlib"]

# The module is linked with \`module\` off: its types, no host import.
[dependencies]
futures.workspace = true
ducktape-view-guest.workspace = true
$program.workspace = true
serde.workspace = true

[dev-dependencies]
serde_json.workspace = true
EOF
    cat > "$dir/src/lib.rs" <<EOF
//! $title: the count the \`$program\` program keeps, re-read on every live
//! bump of the program.
use ducktape_view_guest::methods::{Changes, Program, Query};
use ducktape_view_guest::export_view;
use ducktape_view_guest::host::Refusal;
use ducktape_view_guest::view::Loaded;
use ducktape_view_guest::{
    Context, Host, InteractiveElement, IntoElement, ParentElement, Render,
    StatefulInteractiveElement, Styled, Task, Theme, View, Window, div, px,
};
use futures::StreamExt;
use serde::{Deserialize, Serialize};

/// The program's query surface, as this view reads it.
struct ${title}Program;
impl Program for ${title}Program {
    const NAME: &'static str = $program_snake::MODULE;
    type Op = $program_snake::Op;
    type Query = $program_snake::Query;
    type Reply = $program_snake::Reply;
}

#[derive(Serialize, Deserialize, Default)]
pub struct $title {
    count: Loaded<u64>,
    #[serde(skip)]
    live: Option<Task<()>>,
}

impl View for $title {
    const PREFERRED_WINDOW_SIZE: &'static str = "480,320";

    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut view = Self::default();
        view.restored(window, cx);
        view
    }

    fn restored(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let mut stream = cx.host().subscribe::<Changes<${title}Program>>(());
        self.live = Some(cx.spawn(async move |this, cx| {
            while stream.next().await.is_some() {
                if this.update(cx, |view, cx| view.read(cx)).is_err() {
                    break;
                }
            }
        }));
        self.read(cx);
    }
}

impl Render for $title {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = *cx.global::<Theme>();
        let body = match &self.count {
            Loaded::Idle | Loaded::Loading(_) => "Reading…".to_owned(),
            Loaded::Ready(count) => format!("Count: {count}"),
            Loaded::Failed(refusal) => refusal.sentence.clone(),
        };
        div()
            .id("$program")
            .flex()
            .flex_col()
            .gap_3()
            .p_5()
            .size_full()
            .bg(theme.background)
            .text_color(theme.foreground)
            .text_size(px(13.))
            .child(
                div()
                    .id("$program-title")
                    .text_size(px(16.))
                    .font_weight(ducktape_view_guest::FontWeight::SEMIBOLD)
                    .role(ducktape_view_guest::Role::Heading)
                    .aria_level(1)
                    .child("$title"),
            )
            .child(div().id("$program-body").child(body))
    }
}

impl $title {
    /// One read: the boot, a restore, a live bump. A value already on
    /// screen stays there while it runs.
    fn read(&mut self, cx: &mut Context<Self>) {
        match self.count.ready() {
            Some(_) => cx.refresh(count(cx.host()), |view, count, _| {
                view.count = Loaded::Ready(count)
            }),
            None => self.count = cx.load(count(cx.host()), |view| &mut view.count),
        }
        cx.notify();
    }
}

async fn count(host: Host) -> Result<u64, Refusal> {
    let $program_snake::Reply::Count(count) = host
        .ask::<Query<${title}Program>>($program_snake::Query::Count)
        .await?;
    Ok(count)
}

export_view!(
    $title,
    "$title",
    "The count the $program program keeps.",
    ["program"]
);

#[cfg(test)]
mod tests;
EOF
    cat > "$dir/src/tests.rs" <<EOF
use super::*;
use ducktape_view_guest::testing::TestAppContext;

fn ready() -> TestAppContext {
    let mut cx = TestAppContext::new();
    cx.host().stream::<Changes<${title}Program>>();
    cx.host()
        .handle::<Query<${title}Program>>(|_| Ok($program_snake::Reply::Count(5)));
    cx.open::<$title>();
    cx.run_until_parked();
    cx
}

/// The ready screen; \`${upper}_SCREEN_EXPORT=1\` also writes its tree for the
/// app's node-less renderer (\`ducktape-app --render-tree <json>\`).
#[test]
fn the_ready_screen_shows_the_count() {
    let cx = ready();
    assert!(cx.has_text("Count: 5"), "{:?}", cx.texts());
    cx.assert_accessible();
    if std::env::var_os("${upper}_SCREEN_EXPORT").is_none() {
        return;
    }
    let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../target/$program-screens");
    std::fs::create_dir_all(&out).unwrap();
    std::fs::write(
        out.join("01-ready-light.json"),
        serde_json::to_vec(cx.root()).unwrap(),
    )
    .unwrap();
}
EOF
    register VIEWS "$name"
    cat <<EOF
$dir/{Cargo.toml,src/lib.rs,src/tests.rs}, VIEWS and workspace members.
Next:
  1. \`make dev V=$name\` builds, gates (ABI) and tests it
  2. replace the screen in src/lib.rs with what the program's Query answers
  3. tell qa's kit to pack $snake into $program (crates/view-pack in kit's build)
EOF
}

# Import order and line width follow the name, so rustfmt has the last word.
case "$kind" in module | program) module && ${CARGO:-cargo} fmt -p "$name" ;; view) view && ${CARGO:-cargo} fmt -p "$name" ;; *) echo "usage: tools/scaffold.sh module <name> | view <name>-view" >&2; exit 1 ;; esac
