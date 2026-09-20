//! call's read model: who is in which channel's call — folded from the
//! applied-op feed into call's per-module index database.
//!
//! canonical call state serves the DISPATCH point read (`CallQuery::Roster`)
//! only; the human-facing surface — a channel's seats rendered as handles
//! and hex node keys, and the list of channels with a call going — is served
//! here, where the engine iterates natively. consensus never reads this
//! tier; this tier never feeds consensus.
//!
//! key spaces (inside call's per-module index database):
//! - `roster/{channel}` — one [`RosterRow`] per channel with a live call.
//!   an emptied call deletes the row, so the channel list IS the keyspace:
//!   a prefix scan enumerates exactly the channels with someone in the call.
//!
//! the feed carries two kinds of applied op, told apart the way the module
//! tells them apart — by the authenticated origin: an op from
//! `Origin::Module(chat)` is a chat event (its payload a `ChatEvent`); any
//! other origin's op is a [`CallMsg`] whose stamp ([`CallAssigned`]) names
//! the exact seat the module affected.
//!
//! this file is the DECISION core — pure functions over [`StateRead`],
//! compiled natively and unit-tested against a plain map. the wasm shell
//! (`src/index_guest.rs`, feature `index-guest`) wires it into the engine.

use index_guest::{Fail, OpRow, OriginKind, StateRead, Writes, user_handle};
use serde::{Deserialize, Serialize};

use crate::consumer_wire::chat::{ChatEvent, decode_event};
use crate::{CallMsg, Party, decode_assigned, decode_msg};

/// default and max page size for the channel listing.
const DEFAULT_LIST_LIMIT: usize = 50;
const MAX_LIST_LIMIT: usize = 256;

/// [`Fail`] code: an applied op's payload did not decode — interface drift,
/// which only a refold can honestly repair.
const FAIL_OP_DECODE: i32 = 2;
/// [`Fail`] code: a stored row did not decode — a damaged read model.
const FAIL_ROW_DECODE: i32 = 3;
/// [`Fail`] code: a view request this mapper does not speak.
const FAIL_BAD_REQUEST: i32 = 4;
/// [`Fail`] code: an applied op carried a missing or undecodable assigned
/// stamp — the same interface-drift class as [`FAIL_OP_DECODE`].
const FAIL_ASSIGNED_DECODE: i32 = 5;

/// one seat: rendered party handle plus the hex node key peers route media
/// to.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct SeatRow {
    pub party: String,
    pub node: String,
    pub joined_at: u64,
}

/// the stored row of one channel's call, join order.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct RosterRow {
    pub channel_id: String,
    pub seats: Vec<SeatRow>,
}

/// call's view requests, externally tagged:
/// `{"roster": {"channel_id": "general"}}`.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CallViewQuery {
    /// the seats of one channel's call; empty when no call is going.
    Roster { channel_id: String },
    /// the channels with a call going, ascending by channel id.
    Active {
        #[serde(default)]
        after: Option<String>,
        #[serde(default)]
        limit: Option<usize>,
    },
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ActivePage {
    pub rosters: Vec<RosterRow>,
    pub has_more: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_after: Option<String>,
}

/// call's view replies.
#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CallViewReply {
    Roster(Vec<SeatRow>),
    Active(ActivePage),
}

const ROSTER_PREFIX: &str = "roster/";

fn roster_key(channel: &str) -> String {
    format!("{ROSTER_PREFIX}{channel}")
}

/// the rendered handle a party shows as — the same rendering chat's index
/// uses for members, so one person reads the same in both views.
pub fn party_handle(party: &Party) -> String {
    match party {
        Party::Account(account) => format!("acct:{account}"),
        Party::Key(key) => format!("user:{}", user_handle(key)),
        Party::Module(module) => format!("module:{module}"),
        Party::System => "system".to_string(),
    }
}

/// lowercase hex, the node-key rendering the media plane's routing UI reads.
fn hex_lower(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push_str(&format!("{b:02x}"));
    }
    out
}

fn decode_row(bytes: &[u8]) -> Result<RosterRow, Fail> {
    serde_json::from_slice(bytes).map_err(|e| Fail::new(FAIL_ROW_DECODE, e.to_string()))
}

fn read_roster(read: &impl StateRead, channel: &str) -> Result<Option<RosterRow>, Fail> {
    read.get(roster_key(channel).as_bytes())
        .map(|bytes| decode_row(&bytes))
        .transpose()
}

/// stage the row; an emptied call deletes it.
fn put_roster(out: &mut Writes, row: &RosterRow) -> Result<(), Fail> {
    let key = roster_key(&row.channel_id);
    if row.seats.is_empty() {
        index_guest::delete(out, key);
        return Ok(());
    }
    let bytes = serde_json::to_vec(row).map_err(|e| Fail::new(FAIL_ROW_DECODE, e.to_string()))?;
    index_guest::put(out, key, bytes);
    Ok(())
}

fn is_chat_event(op: &OpRow, chat: &str) -> bool {
    op.origin.kind == OriginKind::Module && op.origin.id.as_deref() == Some(chat)
}

fn fold_chat_event(op: &OpRow, read: &impl StateRead, out: &mut Writes) -> Result<(), Fail> {
    let event = decode_event(&op.payload).map_err(|e| Fail::new(FAIL_OP_DECODE, e))?;
    if let ChatEvent::ChannelArchived { channel_id } = event
        && read_roster(read, &channel_id)?.is_some()
    {
        index_guest::delete(out, roster_key(&channel_id));
    }
    Ok(())
}

fn fold_call(op: &OpRow, read: &impl StateRead, out: &mut Writes) -> Result<(), Fail> {
    let msg = decode_msg(&op.payload).map_err(|e| Fail::new(FAIL_OP_DECODE, e))?;
    let stamp = decode_assigned(&op.assigned).map_err(|e| Fail::new(FAIL_ASSIGNED_DECODE, e))?;
    let seat = party_handle(stamp.participant());
    let channel_id = match &msg {
        CallMsg::Join { channel_id, .. }
        | CallMsg::Leave { channel_id }
        | CallMsg::Sweep { channel_id, .. } => channel_id.clone(),
    };
    let mut row = read_roster(read, &channel_id)?.unwrap_or(RosterRow {
        channel_id,
        seats: Vec::new(),
    });
    match msg {
        CallMsg::Join { node, .. } => {
            let node = hex_lower(&node);
            match row.seats.iter_mut().find(|s| s.party == seat) {
                Some(existing) => {
                    if existing.node == node {
                        return Ok(());
                    }
                    // a re-join moves the seat's node; join order and
                    // joined_at stay, mirroring canonical.
                    existing.node = node;
                }
                None => row.seats.push(SeatRow {
                    party: seat,
                    node,
                    joined_at: op.time,
                }),
            }
        }
        CallMsg::Leave { .. } | CallMsg::Sweep { .. } => {
            let before = row.seats.len();
            row.seats.retain(|s| s.party != seat);
            if row.seats.len() == before {
                return Ok(());
            }
        }
    }
    put_roster(out, &row)
}

/// fold one applied op into derived writes. an applied op passed the
/// module's own validation (a failed op aborts its unit and never reaches
/// the feed), so arms mirror the transition without re-judging it. `chat`
/// is the chat module's id — the origin whose ops are channel events.
pub fn fold_op(op: &OpRow, read: &impl StateRead, chat: &str) -> Result<Writes, Fail> {
    let mut out = Writes::new();
    match is_chat_event(op, chat) {
        true => fold_chat_event(op, read, &mut out)?,
        false => fold_call(op, read, &mut out)?,
    }
    Ok(out)
}

/// serve one materialized-view request.
pub fn serve_view(read: &impl StateRead, req: &[u8]) -> Result<Vec<u8>, Fail> {
    let query: CallViewQuery =
        serde_json::from_slice(req).map_err(|e| Fail::new(FAIL_BAD_REQUEST, e.to_string()))?;
    let reply = match query {
        CallViewQuery::Roster { channel_id } => CallViewReply::Roster(
            read_roster(read, &channel_id)?
                .map(|row| row.seats)
                .unwrap_or_default(),
        ),
        CallViewQuery::Active { after, limit } => {
            let after = after.map(|channel| roster_key(&channel).into_bytes());
            let page = read.scan_page(
                ROSTER_PREFIX.as_bytes(),
                after.as_deref(),
                limit.unwrap_or(DEFAULT_LIST_LIMIT).clamp(1, MAX_LIST_LIMIT),
            );
            let rosters = page
                .entries
                .iter()
                .map(|(_, value)| decode_row(value))
                .collect::<Result<Vec<_>, _>>()?;
            CallViewReply::Active(ActivePage {
                next_after: page
                    .has_more
                    .then(|| rosters.last().map(|row| row.channel_id.clone()))
                    .flatten(),
                rosters,
                has_more: page.has_more,
            })
        }
    };
    serde_json::to_vec(&reply).map_err(|e| Fail::new(FAIL_BAD_REQUEST, e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::consumer_wire::chat::encode_event;
    use crate::{CallAssigned, encode_assigned, encode_msg};
    use index_guest::{OriginTag, apply_to_map};
    use std::collections::BTreeMap;

    type Map = BTreeMap<Vec<u8>, Vec<u8>>;

    fn jess() -> Party {
        Party::Key(b"jess".to_vec())
    }

    fn op(height: u64, msg: &CallMsg, participant: Party) -> OpRow {
        OpRow {
            height,
            seq: 0,
            time: height * 10,
            origin: OriginTag::external("jess"),
            payload: encode_msg(msg),
            assigned: encode_assigned(&CallAssigned::Participant {
                actor: jess(),
                participant,
            }),
        }
    }

    fn fold(map: &mut Map, row: &OpRow) {
        apply_to_map(map, fold_op(row, map, "chat").unwrap());
    }

    fn join(channel: &str, node: u8) -> CallMsg {
        CallMsg::Join {
            channel_id: channel.into(),
            node: vec![node; 32],
            node_proof: Vec::new(),
        }
    }

    fn roster(map: &Map, channel: &str) -> Vec<SeatRow> {
        let reply = serve_view(
            map,
            &serde_json::to_vec(&CallViewQuery::Roster {
                channel_id: channel.into(),
            })
            .unwrap(),
        )
        .unwrap();
        let CallViewReply::Roster(seats) = serde_json::from_slice(&reply).unwrap() else {
            panic!("roster reply")
        };
        seats
    }

    fn active(map: &Map, after: Option<&str>, limit: usize) -> ActivePage {
        let reply = serve_view(
            map,
            &serde_json::to_vec(&CallViewQuery::Active {
                after: after.map(Into::into),
                limit: Some(limit),
            })
            .unwrap(),
        )
        .unwrap();
        let CallViewReply::Active(page) = serde_json::from_slice(&reply).unwrap() else {
            panic!("active reply")
        };
        page
    }

    #[test]
    fn seats_follow_join_rejoin_leave_and_sweep() {
        let mut map = Map::new();
        fold(&mut map, &op(1, &join("g", 0xab), jess()));
        fold(&mut map, &op(2, &join("g", 0xcd), Party::Account(7)));
        let seats = roster(&map, "g");
        assert_eq!(seats.len(), 2);
        assert_eq!(seats[0].party, "user:jess");
        assert_eq!(seats[0].node, "ab".repeat(32));
        assert_eq!(seats[0].joined_at, 10);
        assert_eq!(seats[1].party, "acct:7");

        // a re-join with the same node writes nothing; a new node moves the
        // seat's node and keeps its place.
        assert!(fold_op(&op(3, &join("g", 0xab), jess()), &map, "chat").unwrap().is_empty());
        fold(&mut map, &op(4, &join("g", 0xef), jess()));
        let seats = roster(&map, "g");
        assert_eq!(seats[0].node, "ef".repeat(32));
        assert_eq!(seats[0].joined_at, 10, "rejoin keeps join order");

        // a sweep names the seat through the stamp, not the payload.
        fold(
            &mut map,
            &op(
                5,
                &CallMsg::Sweep {
                    channel_id: "g".into(),
                    party: Party::Account(7),
                },
                Party::Account(7),
            ),
        );
        assert_eq!(roster(&map, "g").len(), 1);
        // an absent seat leaves nothing to write.
        assert!(
            fold_op(
                &op(
                    6,
                    &CallMsg::Leave {
                        channel_id: "g".into()
                    },
                    Party::Account(9)
                ),
                &map,
                "chat"
            )
            .unwrap()
            .is_empty()
        );
        // the last leaver deletes the row: the channel drops off the list.
        fold(
            &mut map,
            &op(
                7,
                &CallMsg::Leave {
                    channel_id: "g".into(),
                },
                jess(),
            ),
        );
        assert!(roster(&map, "g").is_empty());
        assert!(map.is_empty());
    }

    #[test]
    fn active_lists_channels_with_a_call_and_pages() {
        let mut map = Map::new();
        for (height, channel) in ["a", "b", "c"].into_iter().enumerate() {
            fold(&mut map, &op(height as u64 + 1, &join(channel, 1), jess()));
        }
        let page = active(&map, None, 2);
        assert_eq!(
            page.rosters.iter().map(|r| r.channel_id.as_str()).collect::<Vec<_>>(),
            ["a", "b"]
        );
        assert!(page.has_more);
        assert_eq!(page.next_after.as_deref(), Some("b"));
        let page = active(&map, Some("b"), 2);
        assert_eq!(page.rosters.len(), 1);
        assert_eq!(page.rosters[0].channel_id, "c");
        assert!(!page.has_more);
        assert_eq!(page.next_after, None);
    }

    #[test]
    fn a_chat_archive_event_clears_the_roster() {
        let mut map = Map::new();
        fold(&mut map, &op(1, &join("g", 1), jess()));
        let archived = OpRow {
            height: 2,
            seq: 0,
            time: 20,
            origin: OriginTag::module("chat"),
            payload: encode_event(&ChatEvent::ChannelArchived {
                channel_id: "g".into(),
            }),
            assigned: Vec::new(),
        };
        fold(&mut map, &archived);
        assert!(map.is_empty());
        // a second archive of an empty call writes nothing; a post event
        // is not addressed to call.
        assert!(fold_op(&archived, &map, "chat").unwrap().is_empty());
        let posted = OpRow {
            payload: encode_event(&ChatEvent::MessagePosted {
                channel_id: "g".into(),
                seq: 1,
                thread_root: None,
                author: Party::Account(7),
                mentions: Vec::new(),
            }),
            ..archived
        };
        assert!(fold_op(&posted, &map, "chat").unwrap().is_empty());
    }
}
