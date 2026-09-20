//! A separately built application policy. The first network account manages
//! this example's membership; that rule belongs to this guest, never the host.
use ducktape_module_sdk::sdk::refusal;
use ducktape_module_sdk::{Guest, host, rejected};
use serde::{Deserialize, Serialize};

#[derive(Default, Serialize, Deserialize)]
struct State {
    members: Vec<u64>,
    count: u64,
    last: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
enum Operation {
    Configure { members: Vec<u64> },
    Record { text: String },
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
enum Query {
    Authorize { account: u64, text: String },
    State,
}

fn load() -> Result<State, host::Error> {
    host::state_get(b"state")
        .map(|bytes| {
            serde_json::from_slice(&bytes).map_err(|e| rejected(refusal::CORRUPT, e.to_string()))
        })
        .unwrap_or_else(|| Ok(State::default()))
}

/// the record policy, one refusal per way a caller recovers; the `authorize`
/// query answers whether it admits.
fn admit(state: &State, account: u64, text: &str) -> Result<(), host::Error> {
    if !state.members.contains(&account) {
        return Err(rejected(
            refusal::UNAUTHORIZED,
            format!("account {account} is not a member"),
        ));
    }
    if text.is_empty() {
        return Err(rejected(refusal::INVALID_INPUT, "the text is empty"));
    }
    if text.len() > 128 {
        return Err(rejected(
            refusal::CAPACITY,
            format!("a text of {} bytes exceeds the bound of 128", text.len()),
        ));
    }
    if cfg!(feature = "replacement") && !text.starts_with('#') {
        return Err(rejected(
            refusal::INVALID_INPUT,
            "the text does not start with '#'",
        ));
    }
    Ok(())
}

fn authorized(state: &State, account: u64, text: &str) -> bool {
    admit(state, account, text).is_ok()
}

fn caller() -> Result<u64, host::Error> {
    let host::Origin::External(key) = host::get_env().origin else {
        return Err(rejected(
            refusal::UNAUTHORIZED,
            "a user signature is required",
        ));
    };
    let query = serde_json::to_vec(&serde_json::json!({"of_key": {"key": key}}))
        .map_err(|e| rejected(refusal::CORRUPT, e.to_string()))?;
    let answer = host::query_module("identity", &query)?;
    let answer: serde_json::Value = serde_json::from_slice(&answer)
        .map_err(|e| rejected(refusal::UNEXPECTED_REPLY, e.to_string()))?;
    answer["account"]["number"]
        .as_u64()
        .ok_or_else(|| rejected(refusal::NOT_FOUND, "caller has no account"))
}

fn decide(state: State, account: u64, operation: Operation) -> Result<State, host::Error> {
    match operation {
        Operation::Configure { members } => configure(state, account, members),
        Operation::Record { text } => record(state, account, text),
    }
}

fn configure(mut state: State, account: u64, members: Vec<u64>) -> Result<State, host::Error> {
    if account != 1 {
        return Err(rejected(
            refusal::UNAUTHORIZED,
            format!("account {account} does not manage this example's membership"),
        ));
    }
    if members.len() > 16 {
        return Err(rejected(
            refusal::CAPACITY,
            format!("{} members exceed the bound of 16", members.len()),
        ));
    }
    if members.contains(&0) {
        return Err(rejected(
            refusal::INVALID_INPUT,
            "account 0 cannot be a member",
        ));
    }
    state.members = members;
    Ok(state)
}

fn record(mut state: State, account: u64, text: String) -> Result<State, host::Error> {
    admit(&state, account, &text)?;
    state.count = state
        .count
        .checked_add(1)
        .ok_or_else(|| rejected(refusal::EXHAUSTED, "counter full"))?;
    state.last = text;
    Ok(state)
}

struct Component;

impl Guest for Component {
    fn shape() -> host::ModuleShape {
        host::ModuleShape {
            backing: host::Backing::Map,
            config: Vec::new(),
            committed_queries: true,
        }
    }

    fn initialize(_parameters: Vec<u8>) -> Result<(), host::Error> {
        Ok(())
    }

    fn finalize_block() -> Result<(), host::Error> {
        Ok(())
    }

    fn pending_items() -> Result<Vec<host::PendingItem>, host::Error> {
        Ok(Vec::new())
    }

    fn acknowledge(_ack: host::Ack) -> Result<(), host::Error> {
        Err(rejected(refusal::UNSUPPORTED, "no pending work"))
    }

    fn execute(payload: Vec<u8>) -> Result<(), host::Error> {
        let operation: Operation = serde_json::from_slice(&payload)
            .map_err(|e| rejected(refusal::INVALID_INPUT, e.to_string()))?;
        let account = caller()?;
        let state = decide(load()?, account, operation)?;
        host::state_set(
            b"state",
            &serde_json::to_vec(&state).map_err(|e| rejected(refusal::CORRUPT, e.to_string()))?,
        );
        host::emit_event("changed", &[]);
        Ok(())
    }

    fn query(request: Vec<u8>) -> Result<Vec<u8>, host::Error> {
        let query: Query = serde_json::from_slice(&request)
            .map_err(|e| rejected(refusal::INVALID_INPUT, e.to_string()))?;
        let state = load()?;
        match query {
            Query::Authorize { account, text } => {
                serde_json::to_vec(&authorized(&state, account, &text))
                    .map_err(|e| rejected(refusal::CORRUPT, e.to_string()))
            }
            Query::State => {
                serde_json::to_vec(&state).map_err(|e| rejected(refusal::CORRUPT, e.to_string()))
            }
        }
    }
}

ducktape_module_sdk::export_module!(Component);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn membership_and_message_policy_are_owned_by_the_deployed_code() {
        let state = State {
            members: vec![1, 9],
            ..Default::default()
        };
        assert!(authorized(&state, 9, "#hello"));
        assert!(!authorized(&state, 8, "#hello"));
        assert!(!authorized(&state, 9, ""));
        assert!(!authorized(&state, 9, &"#".repeat(129)));
        assert_eq!(
            authorized(&state, 9, "hello"),
            !cfg!(feature = "replacement")
        );
    }

    /// each check refuses with the class its caller recovers by.
    #[test]
    fn a_refusal_names_how_the_caller_recovers() {
        fn class(refused: Result<impl Sized, host::Error>) -> String {
            let Err(host::Error::Rejected(framed)) = refused else {
                panic!("expected a refusal");
            };
            refusal::decode(&framed).expect("framed").0.to_string()
        }
        let state = State {
            members: vec![1, 9],
            ..Default::default()
        };
        assert_eq!(class(admit(&state, 8, "#hi")), refusal::UNAUTHORIZED);
        assert_eq!(class(admit(&state, 9, "")), refusal::INVALID_INPUT);
        assert_eq!(class(admit(&state, 9, &"#".repeat(129))), refusal::CAPACITY);
        if cfg!(feature = "replacement") {
            assert_eq!(class(admit(&state, 9, "hi")), refusal::INVALID_INPUT);
        }
        assert_eq!(
            class(configure(State::default(), 2, vec![9])),
            refusal::UNAUTHORIZED
        );
        assert_eq!(
            class(configure(State::default(), 1, vec![9; 17])),
            refusal::CAPACITY
        );
        assert_eq!(
            class(configure(State::default(), 1, vec![0])),
            refusal::INVALID_INPUT
        );
    }
}
