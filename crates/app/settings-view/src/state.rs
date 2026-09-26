//! The state Settings keeps: what it read, and each form as typed.
use ducktape_view_guest::Task;
use ducktape_view_guest::view::Loadable;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use crate::api::{Invite, NodeStatus, Session};
use crate::queries::Account;

/// The invite lifetimes offered, in days.
pub(crate) const TTL: [u64; 3] = [1, 7, 30];

#[derive(Default, Serialize, Deserialize)]
pub struct Settings {
    pub(crate) session: Session,
    pub(crate) status: Loadable<NodeStatus>,
    pub(crate) account: Loadable<Option<Account>>,
    pub(crate) invite: Loadable<Invite>,
    /// which of [`TTL`] the next invite lasts
    pub(crate) ttl: usize,
    /// the last copy of the invite: done, or the refusal it met
    pub(crate) copied: Loadable<()>,
    pub(crate) create_account: Form,
    pub(crate) create_agent: Form,
    pub(crate) agent_key: Form,
    /// each agent's rename, by its number
    pub(crate) rename_agent: BTreeMap<u64, Form>,
    /// the one suspend, resume or revoke in flight
    pub(crate) agent_standing: Form,
    /// the agent whose revoke waits for a second press: revoking is final
    pub(crate) revoking: Option<u64>,
    /// What the view follows (`watch.rs`); dropping them unsubscribes.
    #[serde(skip)]
    pub(crate) followers: Vec<Task<()>>,
}

/// A one-field form: what was typed, whether its submit is in flight, and
/// what stopped it.
#[derive(Clone, Default, Serialize, Deserialize)]
pub(crate) struct Form {
    pub(crate) text: String,
    pub(crate) busy: bool,
    pub(crate) problem: Option<Problem>,
}

/// Why a form's submit did not land. The form that shows it words it.
#[derive(Clone, Serialize, Deserialize)]
pub(crate) enum Problem {
    /// nothing was typed
    Empty,
    /// the text is not an `AddKey` for one of the reader's agents
    NotAKeyRequest,
    /// the program refused it, in its own words
    Refused(String),
}
