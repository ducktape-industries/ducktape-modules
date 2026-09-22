#!/bin/sh
# `make new-program NAME=x` / `make new-view NAME=x-view`: a program in
# chat's shape (types and rules always built, the wasm32 program behind
# `program`, a native test over `store::Memory`) or a view in members-view's
# shape (links its program with `program` off, `export_view!`, one screen
# test), registered in the Makefile and the workspace. Run from the repo root.
#   tools/scaffold.sh program <name> | view <name>-view
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

program() {
    mkdir -p "$dir/src" "$dir/tests"
    cat > "$dir/Cargo.toml" <<EOF
[package]
name = "$name"
version.workspace = true
edition.workspace = true

# The types and rules are always built; the view links them with \`program\`
# off. \`program\` adds the wasm32 program over the host: \`guest\`'s contexts,
# the \`alloc\`/\`call\` exports and the \`ducktape.*\` imports.
[lib]
crate-type = ["cdylib", "rlib"]

[features]
program = ["dep:guest", "store/program"]

[dependencies]
abi = { workspace = true }
borsh = { workspace = true }
guest = { workspace = true, optional = true }
store = { workspace = true }
EOF
    cat > "$dir/src/lib.rs" <<EOF
//! The \`$name\` program: one counter, to be replaced by what it keeps.
//!
//! Writes are an [\`Op\`] (borsh), reads a [\`Query\`] answered by a [\`Reply\`]
//! (borsh); \`$name-view\` links the same types. The rules run over any
//! [\`store::Reads\`]/[\`store::Writes\`] store; the \`program\` feature adds the
//! wasm32 program over the host (\`program.rs\`), which a view never enables.
use abi::{Env, Refusal};
use borsh::{BorshDeserialize, BorshSerialize};
use store::{Item, Reads, Writes};

#[cfg(feature = "program")]
mod program;

pub const PROGRAM: &str = "$name";

/// The one value this program keeps.
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

pub fn execute(store: &mut impl Writes, _env: &Env, op: Op) -> Result<(), Refusal> {
    match op {
        Op::Bump { by } => {
            COUNT.update(store, |count| *count = count.saturating_add(by))?;
        }
    }
    Ok(())
}

pub fn query(store: &impl Reads, _env: &Env, query: Query) -> Result<Reply, Refusal> {
    match query {
        Query::Count => Ok(Reply::Count(COUNT.get(store)?.unwrap_or_default())),
    }
}
EOF
    cat > "$dir/src/program.rs" <<EOF
// The wasm32 program over the rules: guest contexts as the store, the bytes decoded and answered.

use abi::{Env, Refusal};
use guest::{Execute, Program, Query as QueryCtx};
use store::decoded;

use crate::{Op, PROGRAM, Query};

struct This;

impl Program for This {
    fn execute(ctx: &mut Execute, env: &Env, payload: &[u8]) -> Result<(), Refusal> {
        crate::execute(ctx, env, decoded::<Op>(PROGRAM, "Op", payload)?)
    }

    fn query(ctx: &mut QueryCtx, env: &Env, request: &[u8]) -> Result<(), Refusal> {
        let reply = crate::query(ctx, env, decoded::<Query>(PROGRAM, "Query", request)?)?;
        ctx.reply(&reply);
        Ok(())
    }
}

guest::program!(This);
EOF
    cat > "$dir/tests/$snake.rs" <<EOF
use abi::{Cause, Env, Origin};
use store::Memory;
use $snake::{Op, Query, Reply};

fn env() -> Env {
    Env {
        network: b"net".to_vec(),
        height: 7,
        time: 100,
        me: $snake::PROGRAM.into(),
        origin: Origin::External(vec![1]),
        cause: Cause::Direct,
    }
}

#[test]
fn bumps_add_up_and_read_back() {
    let mut store = Memory::default();
    $snake::execute(&mut store, &env(), Op::Bump { by: 2 }).unwrap();
    $snake::execute(&mut store, &env(), Op::Bump { by: 3 }).unwrap();
    let Reply::Count(count) = $snake::query(&store, &env(), Query::Count).unwrap();
    assert_eq!(count, 5);
}
EOF
    register PROGRAMS "$name"
    sed -i "s|^forge = { path = \"crates/app/forge\" }|&\n$name = { path = \"$dir\" }|" Cargo.toml
    cat <<EOF
$dir/{Cargo.toml,src/lib.rs,src/program.rs,tests/$snake.rs}, PROGRAMS, workspace members and dependencies.
Next:
  1. name \`$name\` in a founding (qa's founding.toml, or the program's params it seats with)
  2. write the contract: replace Op/Query/Reply and the rules in src/lib.rs; \`make dev P=$name\`
  3. tell qa's kit about it (the pack step in kit's build, if it ships a view)
EOF
}

view() {
    case "$name" in *-view) ;; *) echo "$name: a view is named <program>-view" >&2; exit 1 ;; esac
    program=${name%-view}
    program_snake=$(echo "$program" | tr - _)
    test -d "crates/app/$program" || { echo "crates/app/$program is not there: make new-program NAME=$program first" >&2; exit 1; }
    upper=$(echo "$program_snake" | tr a-z A-Z)
    # The view type: TitleCase of the program name.
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

# The program is linked with \`program\` off: its types, no host import.
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
use ducktape_view_guest::doors::{Live, Program, Query};
use ducktape_view_guest::export_view;
use ducktape_view_guest::host::Refusal;
use ducktape_view_guest::view::Loaded;
use ducktape_view_guest::{
    Context, Host, InteractiveElement, IntoElement, ParentElement, Render, Styled, Task, Theme,
    View, Window, div, px,
};
use futures::StreamExt;
use serde::{Deserialize, Serialize};

/// The program's query surface, as this view reads it.
struct ${title}Program;
impl Program for ${title}Program {
    const NAME: &'static str = $program_snake::PROGRAM;
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
        let mut stream = cx.host().subscribe::<Live>($program_snake::PROGRAM.into());
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
    ["rpc", "host"]
);

#[cfg(test)]
mod tests;
EOF
    cat > "$dir/src/tests.rs" <<EOF
use super::*;
use ducktape_view_guest::testing::TestAppContext;

fn ready() -> TestAppContext {
    let mut cx = TestAppContext::new();
    cx.host().stream::<Live>();
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
  1. \`make dev V=$name\` builds, gates (ABI, 1,200,000 bytes) and tests it; a larger view needs LIMIT_$name in the Makefile
  2. replace the screen in src/lib.rs with what the program's Query answers
  3. tell qa's kit to pack $snake into $program (crates/view-pack in kit's build)
EOF
}

case "$kind" in program | view) "$kind" ;; *) echo "usage: tools/scaffold.sh program <name> | view <name>-view" >&2; exit 1 ;; esac
