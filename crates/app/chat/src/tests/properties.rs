//! Random walks of ops by three accounts: whatever order they come in, a
//! refused op writes nothing and every index agrees with the messages.
use super::channels::edit;
use super::*;
use crate::state::{HEADS, MESSAGES, REACTIONS, TAGS, WORDS};
use crate::{MsgRow, tokens};

/// xorshift64*: the same seed walks the same way.
struct Rng(u64);

impl Rng {
    fn below(&mut self, n: u64) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D) % n.max(1)
    }

    fn pick<'a, T>(&mut self, from: &'a [T]) -> &'a T {
        &from[self.below(from.len() as u64) as usize]
    }
}

const TEXTS: [&str; 4] = ["alpha beta", "beta #ship", "gamma #ship #it", "alpha"];
const EMOJI: [&str; 3] = ["👍", "🎉", "👀"];

#[test]
fn any_walk_keeps_every_index_in_step_with_the_messages() {
    for seed in 1..=24 {
        let mut rng = Rng(seed);
        let mut chat = Chat::with_channel(PostPolicy::Open);
        for step in 0..80 {
            let head = HEADS.get(&chat.store, &"general".into()).unwrap();
            let seq = 1 + rng.below(head.unwrap_or(0));
            let text = *rng.pick(&TEXTS);
            let op = match rng.below(6) {
                0 => post("general", &format!("m{step}"), text, None),
                1 => post("general", &format!("m{step}"), text, Some(seq)),
                2 => edit(seq, text),
                3 => delete(seq),
                n => react(seq, rng.pick(&EMOJI), n == 4),
            };
            let who = rng.pick(&[ADA, BO, CY]).clone();
            let before = chat.store.state.clone();
            if chat.run(&who, op).is_err() {
                assert_eq!(chat.store.state, before, "seed {seed}: a refused op wrote");
            }
        }
        in_step(&chat, seed);
    }
}

fn in_step(chat: &Chat, seed: u64) {
    let rows: Vec<MsgRow> = MESSAGES
        .all(&chat.store)
        .unwrap()
        .into_iter()
        .map(|(_, row)| row)
        .collect();
    let reactions = REACTIONS.all(&chat.store).unwrap();
    for row in &rows {
        let chosen = |emoji: &str| {
            reactions
                .iter()
                .filter(|(ch, seq, e, _)| *ch == row.channel_id && *seq == row.seq && e == emoji)
                .count() as u64
        };
        let counted: u64 = row.reactions.iter().map(|r| r.count).sum();
        let marked = reactions
            .iter()
            .filter(|(_, seq, _, _)| *seq == row.seq)
            .count() as u64;
        assert_eq!(
            counted, marked,
            "seed {seed}: #{} counts its markers",
            row.seq
        );
        for reaction in &row.reactions {
            assert_eq!(reaction.count, chosen(&reaction.emoji), "seed {seed}");
        }
        if row.deleted {
            assert_eq!(marked, 0, "seed {seed}: a tombstone keeps no reactions");
        }
    }
    let row = |seq: u64| rows.iter().find(|row| row.seq == seq).unwrap();
    let words = WORDS.all(&chat.store).unwrap();
    for (word, _, seq) in &words {
        assert!(
            tokens(&row(*seq).text).contains(word),
            "seed {seed}: a stale posting"
        );
    }
    let tags = TAGS.all(&chat.store).unwrap();
    for (tag, _, _, seq) in &tags {
        assert!(row(*seq).tags.contains(tag), "seed {seed}: a stale tag");
    }
    for row in rows.iter().filter(|row| !row.deleted) {
        let posted = |word: &String| words.iter().any(|(w, _, s)| w == word && *s == row.seq);
        assert!(
            tokens(&row.text).iter().all(posted),
            "seed {seed}: a missing posting"
        );
        let tagged = |tag: &String| tags.iter().any(|(t, _, _, s)| t == tag && *s == row.seq);
        assert!(row.tags.iter().all(tagged), "seed {seed}: a missing tag");
    }
}
