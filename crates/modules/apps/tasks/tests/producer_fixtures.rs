//! Frozen producer bytes captured before the move from SDK b66.

use tasks::{
    JobsEvent, Party, Task, TaskMsg, TaskQuery, TaskReply, TaskStatus, WorkAssigned, WorkMsg,
    WorkQuery, WorkReply, decode_assigned, decode_job_event, decode_work_msg, decode_work_query,
    decode_work_reply, encode_assigned, encode_job_event, encode_work_msg, encode_work_query,
    encode_work_reply,
};

fn bytes(hex: &str) -> Vec<u8> {
    hex.trim()
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
        .collect()
}

// Source: ducktape-sdk b66f47f1f4b0c869786ce195e382f2e83fd15277.
// Requests, queries, replies, events and assignments use sdk::wire JSON;
// task.json.hex is the module's stored JSON Task record.
#[test]
fn b66_producer_encodings_are_immutable_and_decode() {
    let msg = WorkMsg::Task(TaskMsg::CreateTask {
        task_id: "t1".into(),
        title: "Ship".into(),
        owner: Some(7),
    });
    let query = WorkQuery::Task(TaskQuery::Get {
        task_id: "t1".into(),
    });
    let task = Task {
        id: "t1".into(),
        title: "Ship".into(),
        status: TaskStatus::Open,
        owner: Party::Account(7),
        created_at: 1,
        updated_at: 2,
    };
    let reply = WorkReply::Task(TaskReply::Task(Some(task.clone())));
    let event = JobsEvent::Submitted {
        job_id: "j1".into(),
        kind: "kind".into(),
        submitter: Party::Account(7),
        spec: "spec".into(),
        spec_hash: vec![1, 2, 3],
    };
    let assigned = WorkAssigned::Job {
        actor: Party::Module("worker".into()),
    };
    let msg_bytes = bytes(include_str!("fixtures/request.json.hex"));
    let query_bytes = bytes(include_str!("fixtures/query.json.hex"));
    let reply_bytes = bytes(include_str!("fixtures/reply.json.hex"));
    let event_bytes = bytes(include_str!("fixtures/event.json.hex"));
    let assigned_bytes = bytes(include_str!("fixtures/assigned.json.hex"));
    assert_eq!(encode_work_msg(&msg), msg_bytes);
    assert_eq!(encode_work_query(&query), query_bytes);
    assert_eq!(encode_work_reply(&reply), reply_bytes);
    assert_eq!(encode_job_event(&event), event_bytes);
    assert_eq!(encode_assigned(&assigned), assigned_bytes);
    assert_eq!(decode_work_msg(&msg_bytes).unwrap(), msg);
    assert_eq!(decode_work_query(&query_bytes).unwrap(), query);
    assert_eq!(decode_work_reply(&reply_bytes).unwrap(), reply);
    assert_eq!(decode_job_event(&event_bytes).unwrap(), event);
    assert_eq!(decode_assigned(&assigned_bytes).unwrap(), assigned);
    assert_eq!(
        serde_json::to_vec(&task).unwrap(),
        bytes(include_str!("fixtures/task.json.hex"))
    );
    assert!(decode_work_msg(&msg_bytes[..msg_bytes.len() - 1]).is_err());
}
