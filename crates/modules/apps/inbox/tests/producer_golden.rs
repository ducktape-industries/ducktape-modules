//! Golden producer proof. Fixtures were captured before this relocation from
//! SDK `b66f47f1f4b0c869786ce195e382f2e83fd15277`'s `inbox-wire`;
//! JSON uses `sdk::wire`, Notification/ChangeRef storage uses canonical Borsh.
use attribution::{Actor, ChangeKind, ChangeRef, Reason, Source};
use borsh::{from_slice, to_vec};
use inbox::{
    InboxAssigned, InboxMsg, Notification, decode_assigned, decode_msg, encode_assigned, encode_msg,
};
use sdk::Cause;

fn fixture(text: &str) -> Vec<u8> {
    let text = text.trim();
    (0..text.len())
        .step_by(2)
        .map(|at| u8::from_str_radix(&text[at..at + 2], 16).unwrap())
        .collect()
}

fn change() -> ChangeRef {
    ChangeRef {
        seq: 3,
        source: Source {
            module: "chat".into(),
            kind: "message".into(),
            object: "m1".into(),
        },
        revision: 2,
        recipient: 7,
        reason: Reason::Mention,
        kind: ChangeKind::Added,
        actor: Actor::Account(7),
        cause: Cause::Direct,
        height: 9,
    }
}

#[test]
fn producer_encodings_match_frozen_bytes() {
    let notification = Notification {
        seq: 1,
        account: 7,
        change: change(),
        created_at: 10,
    };
    let msg = InboxMsg::MarkRead {
        account: 7,
        up_to_seq: 1,
    };
    let assigned = InboxAssigned::Delivered { seq: 1 };
    let msg_bytes = fixture(include_str!("fixtures/msg.hex"));
    let assigned_bytes = fixture(include_str!("fixtures/assigned.hex"));
    let notification_bytes = fixture(include_str!("fixtures/notification-borsh.hex"));
    let change_bytes = fixture(include_str!("fixtures/change-borsh.hex"));

    assert_eq!(encode_msg(&msg), msg_bytes);
    assert_eq!(decode_msg(&msg_bytes).unwrap(), msg);
    assert_eq!(encode_assigned(&assigned), assigned_bytes);
    assert_eq!(decode_assigned(&assigned_bytes).unwrap(), assigned);
    assert_eq!(to_vec(&notification).unwrap(), notification_bytes);
    assert_eq!(
        from_slice::<Notification>(&notification_bytes).unwrap(),
        notification
    );
    assert_eq!(to_vec(&notification.change).unwrap(), change_bytes);
    assert_eq!(
        from_slice::<ChangeRef>(&change_bytes).unwrap(),
        notification.change
    );
}

#[test]
fn producer_rejects_malformed_fixture() {
    let malformed = fixture(include_str!("fixtures/malformed.hex"));
    assert!(decode_msg(&malformed).is_err());
    assert!(decode_assigned(&malformed).is_err());
}
