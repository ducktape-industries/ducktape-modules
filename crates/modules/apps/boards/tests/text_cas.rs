//! The text compare-and-set at the module boundary, pinned to what the live
//! boards#2 acceptance proved (backend cases 5.2–5.13 and N1, GUI G1–G6). A
//! refusal here is checked for the whole of what it must leave alone: the
//! board document (its revision included), the catalogue, and the committed
//! boards root after the block is committed anyway.
use boards::*;
use futures::executor::block_on;
use sdk::{Ctx, Module, Msg, Origin, StateRoot};
use sdk_testkit::{MemStore, TestCtx};
use std::collections::BTreeMap;

// Refusal tokens, in one place: the sdk refusal classes, plus the one class a
// board adds (`TARGET_GONE`, from boards-wire through the glob import).
const STALE: &str = sdk::refusal::STALE;
const BOARD_GONE: &str = sdk::refusal::NOT_FOUND;
const CODEC: &str = sdk::refusal::INVALID_INPUT;

/// The real module over an in-memory store, holding board `room` with one
/// card, `card`, committed.
struct Room {
    module: Boards,
    ctx: TestCtx,
}
impl Room {
    async fn with_card() -> Self {
        let mut env = TestCtx::at_height(1).env().clone();
        env.origin = Origin::External(vec![7; 32]);
        let mut room = Self {
            module: Boards::new(Box::new(MemStore::new())),
            ctx: TestCtx::with_env(env),
        };
        room.apply(Operation::Create {
            id: "room".into(),
            title: "Planning".into(),
        })
        .await;
        room.apply(edit(Change::Create {
            id: "card".into(),
            shape: Shape::default(),
        }))
        .await;
        room
    }
    async fn submit(&mut self, payload: Vec<u8>) -> Result<(), sdk::Error> {
        let msg = Msg {
            target: "boards".into(),
            payload,
        };
        self.module.execute(&mut self.ctx, &msg).await
    }
    /// Submits an operation that must land, and commits it.
    async fn apply(&mut self, operation: Operation) {
        self.submit(serde_json::to_vec(&operation).unwrap())
            .await
            .unwrap();
        self.module.commit_block().await.unwrap();
    }
    /// Submits a payload that must be refused, commits the block regardless,
    /// and proves the refusal left no trace before handing back its reason
    /// and sentence.
    async fn refused_raw(&mut self, payload: Vec<u8>) -> (String, String) {
        let before = self.state().await;
        let error = self
            .submit(payload)
            .await
            .expect_err("the module took a write it must refuse");
        self.module.commit_block().await.unwrap();
        assert_eq!(self.state().await, before, "a refused op left a trace");
        match error {
            sdk::Error::Module { reason, sentence } => (reason.to_string(), sentence),
            other => panic!("not a module refusal: {other:?}"),
        }
    }
    async fn refused(&mut self, operation: Operation) -> (String, String) {
        self.refused_raw(serde_json::to_vec(&operation).unwrap())
            .await
    }
    /// Everything a refusal must leave alone.
    async fn state(&self) -> (Option<Board>, BTreeMap<String, String>, StateRoot) {
        (
            self.board().await,
            self.catalogue().await,
            self.module.root(),
        )
    }
    async fn board(&self) -> Option<Board> {
        let request = serde_json::to_vec(&Query::Get { id: "room".into() }).unwrap();
        let Reply::Board(board) =
            serde_json::from_slice(&self.module.query(&request).await.unwrap()).unwrap()
        else {
            panic!("board reply")
        };
        board
    }
    async fn card(&self, id: &str) -> Record {
        self.board().await.unwrap().shapes[id].clone()
    }
    async fn catalogue(&self) -> BTreeMap<String, String> {
        let request = serde_json::to_vec(&Query::List).unwrap();
        let Reply::List(catalogue) =
            serde_json::from_slice(&self.module.query(&request).await.unwrap()).unwrap()
        else {
            panic!("list reply")
        };
        catalogue
    }
}
fn edit(change: Change) -> Operation {
    Operation::Edit {
        board: "room".into(),
        change,
    }
}
fn batch(changes: Vec<Change>) -> Operation {
    Operation::Batch {
        board: "room".into(),
        changes,
    }
}
fn text(id: &str, words: &str, base_revision: u64) -> Change {
    Change::Text {
        id: id.into(),
        text: words.into(),
        base_revision,
    }
}
fn move_to(x: i32, y: i32) -> Change {
    Change::Move {
        id: "card".into(),
        x,
        y,
    }
}
fn resize(width: i32, height: i32) -> Change {
    Change::Resize {
        id: "card".into(),
        width,
        height,
    }
}
fn refusal(reason: &str, sentence: &str) -> (String, String) {
    (reason.into(), sentence.into())
}

/// G5 sent `[Text, Resize]` and backend 5.7 sent `[Move, Text]`: either way
/// round, a stale Text takes every other change in its batch down with it.
#[test]
fn a_stale_text_refuses_its_whole_batch_in_either_order() {
    block_on(async {
        let mut room = Room::with_card().await;
        let held = room.card("card").await.revision;
        // Somebody else's words land first (5.2), carrying everything the
        // stale sentence has to hand back verbatim.
        let theirs = "theirs — \"quoted\" \\ ünï 🦆: colon\nsecond line";
        room.apply(edit(text("card", theirs, held))).await;
        let current = room.card("card").await.revision;

        // G5: the writer still holds the revision under their draft, and the
        // card grew while they typed.
        assert_eq!(
            room.refused(batch(vec![text("card", "mine", held), resize(200, 264)]))
                .await,
            refusal(STALE, theirs)
        );
        // 5.7: the base is current when sent, but the move ahead of it in the
        // same batch re-stamps the card first.
        assert_eq!(
            room.refused(batch(vec![move_to(60, 80), text("card", "mine", current)]))
                .await,
            refusal(STALE, theirs)
        );
    });
}

/// 5.6 and G2: a write to any other field re-stamps the card, so words written
/// over the pre-write revision are stale although nobody touched them, and the
/// sentence is those same unchanged words.
#[test]
fn every_shape_write_moves_the_card_on_so_a_pre_write_text_is_stale() {
    block_on(async {
        let mut room = Room::with_card().await;
        let created = room.card("card").await.revision;
        room.apply(edit(text("card", "fresh", created))).await;
        let id = || String::from("card");
        for write in [
            move_to(30, 40),
            resize(240, 180),
            Change::Color { id: id(), color: 3 },
            Change::Fill {
                id: id(),
                fill: Fill::None,
            },
            Change::Dash {
                id: id(),
                dash: Dash::Dashed,
            },
            Change::Weight {
                id: id(),
                weight: Weight::Heavy,
            },
            Change::Align {
                id: id(),
                align: Align::End,
            },
            Change::TextSize {
                id: id(),
                text_size: TextSize::Huge,
            },
            Change::Group {
                ids: vec![id()],
                group: Some("pair".into()),
            },
        ] {
            let held = room.card("card").await.revision;
            room.apply(edit(write.clone())).await;
            let card = room.card("card").await;
            assert!(card.revision > held, "{write:?} left the revision alone");
            assert_eq!(card.shape.text, "fresh", "{write:?} touched the words");
            assert_eq!(
                room.refused(edit(text("card", "after", held))).await,
                refusal(STALE, "fresh"),
                "{write:?}"
            );
        }
    });
}

/// 5.5, 5.8, G2's and G5's consent: words over the revision the writer read
/// land, keep every other field, and move the card on by one — so the same
/// write sent twice cannot land twice.
#[test]
fn a_text_over_the_current_revision_lands_and_moves_the_revision_on() {
    block_on(async {
        let mut room = Room::with_card().await;
        room.apply(edit(move_to(30, 40))).await;
        let moved = room.board().await.unwrap();
        let root = room.module.root();
        let base = moved.shapes["card"].revision;
        room.apply(edit(text("card", "after", base))).await;
        let written = room.board().await.unwrap();
        let card = &written.shapes["card"];
        assert_eq!(
            (card.shape.text.as_str(), card.shape.x, card.shape.y),
            ("after", 30, 40)
        );
        assert_eq!(written.revision, moved.revision + 1);
        assert_eq!(card.revision, written.revision);
        assert_ne!(room.module.root(), root);
        assert_eq!(
            room.refused(edit(text("card", "after", base))).await,
            refusal(STALE, "after")
        );

        // 5.8: in one batch, over the revision the move ahead of it leaves.
        let base = room.board().await.unwrap().revision + 1;
        room.apply(batch(vec![move_to(60, 80), text("card", "batched", base)]))
            .await;
        let card = room.card("card").await;
        assert_eq!(
            (card.shape.text.as_str(), card.shape.x, card.shape.y),
            ("batched", 60, 80)
        );
        assert_eq!(card.revision, base + 1);

        // G5's consent: the words and the grown box land together.
        room.apply(batch(vec![
            text("card", "tall", card.revision),
            resize(200, 264),
        ]))
        .await;
        let tall = room.card("card").await;
        assert_eq!((tall.shape.text.as_str(), tall.shape.height), ("tall", 264));
        assert_eq!(tall.revision, card.revision + 2);
    });
}

/// 5.9, 5.10 and G3: words for a card that is gone are refused rather than
/// silently dropped, and never put the card back.
#[test]
fn a_text_on_a_deleted_card_is_refused_and_does_not_bring_it_back() {
    block_on(async {
        let mut room = Room::with_card().await;
        let held = room.card("card").await.revision;
        let delete = || Change::Delete { id: "card".into() };
        let gone = refusal(TARGET_GONE, "That card is no longer on the board.");
        // 5.9: deleted ahead of it in the same batch, so neither lands.
        assert_eq!(
            room.refused(batch(vec![delete(), text("card", "late", held)]))
                .await,
            gone
        );
        // 5.10: deleted first, in a block of its own.
        room.apply(edit(delete())).await;
        assert_eq!(room.refused(edit(text("card", "late", held))).await, gone);
        assert!(room.board().await.unwrap().shapes.is_empty());
    });
}

/// N1b and G6: an edit to a removed board is refused, and neither the board
/// nor its catalogue entry comes back.
#[test]
fn an_edit_on_a_removed_board_is_refused_and_the_board_stays_gone() {
    block_on(async {
        let mut room = Room::with_card().await;
        let held = room.card("card").await.revision;
        room.apply(edit(Change::Delete { id: "card".into() })).await;
        room.apply(Operation::Remove {
            board: "room".into(),
        })
        .await;
        // N1b's late text, and G6's late note — a create, having no card left.
        let note = Change::Create {
            id: "note".into(),
            shape: Shape::default(),
        };
        for late in [edit(text("card", "late", held)), batch(vec![note])] {
            assert_eq!(
                room.refused(late).await,
                refusal(BOARD_GONE, "Board no longer exists.")
            );
        }
        assert_eq!(room.board().await, None);
        assert!(room.catalogue().await.is_empty());
    });
}

/// 5.11: `base_revision` is mandatory on the wire, so a text without one does
/// not decode, rather than being taken over some default revision.
#[test]
fn a_text_without_a_base_revision_is_a_codec_refusal() {
    block_on(async {
        let mut room = Room::with_card().await;
        let bare = serde_json::json!({ "text": { "id": "card", "text": "old" } });
        for payload in [
            serde_json::json!({ "edit": { "board": "room", "change": bare } }),
            serde_json::json!({ "batch": { "board": "room", "changes": [bare] } }),
        ] {
            let (reason, sentence) = room
                .refused_raw(serde_json::to_vec(&payload).unwrap())
                .await;
            assert_eq!(reason, CODEC);
            assert!(
                sentence.starts_with("missing field `base_revision`"),
                "{sentence}"
            );
        }
    });
}

/// G3's recovery puts the words on a new card under a new id. That card's
/// revision is its own creation, not an inheritance: the revision held for
/// the deleted card is stale against it, and the deleted id stays gone. The
/// board's revision never goes back, so re-making the SAME id cannot match an
/// old base either.
#[test]
fn a_card_made_again_starts_its_own_revision() {
    block_on(async {
        let mut room = Room::with_card().await;
        let held = room.card("card").await.revision;
        room.apply(edit(Change::Delete { id: "card".into() })).await;
        room.apply(edit(Change::Create {
            id: "rescued".into(),
            shape: Shape {
                text: "late3".into(),
                ..Shape::default()
            },
        }))
        .await;
        let board = room.board().await.unwrap();
        let rescued = &board.shapes["rescued"];
        assert_eq!(
            (rescued.created, rescued.revision),
            (board.revision, board.revision)
        );
        assert!(rescued.revision > held);

        assert_eq!(
            room.refused(edit(text("card", "mine", held))).await,
            refusal(TARGET_GONE, "That card is no longer on the board.")
        );
        assert_eq!(
            room.refused(edit(text("rescued", "mine", held))).await,
            refusal(STALE, "late3")
        );
        room.apply(edit(text("rescued", "mine", rescued.revision)))
            .await;
        let written = room.card("rescued").await;
        assert_eq!(
            (written.shape.text.as_str(), written.revision),
            ("mine", rescued.revision + 1)
        );

        room.apply(edit(Change::Create {
            id: "card".into(),
            shape: Shape::default(),
        }))
        .await;
        assert_eq!(
            room.refused(edit(text("card", "mine", held))).await,
            refusal(STALE, "")
        );
    });
}
