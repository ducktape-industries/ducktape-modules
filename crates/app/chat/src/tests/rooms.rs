//! How an op reads in the explorer (`describe`).
use super::channels::open_dm;
use super::*;
use crate::{describe, dm_channel_id};

#[test]
fn a_dm_title_names_no_account_numbers_and_its_accounts_are_fields() {
    use describe::Value;
    let dm = dm_channel_id(1, 3);
    let react = describe(&Op::AddReaction {
        channel_id: dm.clone(),
        seq: 4,
        emoji: "👍".into(),
    });
    assert_eq!(react.title, "React · DM");
    let between = react.fields.iter().find(|f| f.label == "between").unwrap();
    assert_eq!(
        between.value,
        Value::List(vec![Value::Account(1), Value::Account(3)])
    );
    let edit = describe(&Op::EditMessage {
        channel_id: dm,
        seq: 4,
        blocks: Vec::new(),
        base_rev: None,
    });
    assert_eq!(edit.title, "Edit message · DM");
    // a channel keeps the name a person picked, and no `between`
    let channel = describe(&Op::DeleteMessage {
        channel_id: "old-launch".into(),
        seq: 1,
    });
    assert_eq!(channel.title, "Delete message · #old-launch");
    assert!(channel.fields.iter().all(|f| f.label != "between"));
    assert_eq!(describe(&open_dm(3)).title, "Open a DM");
    // a created channel is titled by the name a person picked, not its id
    let created = describe(&Op::CreateChannel {
        channel_id: "channel-18d8db5b".into(),
        name: "[qa] probe".into(),
        post_policy: PostPolicy::Open,
    });
    assert_eq!(created.title, "Create channel · #[qa] probe");
    assert_eq!(
        describe(&post("design", "m", "hi", None)).title,
        "Post in #design"
    );
}
