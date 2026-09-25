//! What the network calls a principal, as the composer offers and inserts it.
//! The roster itself is [`chat::view::Names`], shared with every view that
//! names chat principals.
use chat::Principal;
use chat::view::Names;

/// Autocomplete candidates: every named account, then the room's
/// account members, labelled without the `@`.
pub fn mention_choices(names: &Names, members: &[Principal]) -> Vec<MentionChoice> {
    let accounts = names.numbers().map(Principal::Account);
    let members = members.iter().filter(|principal| principal.is_person());
    let mut choices: Vec<MentionChoice> = Vec::new();
    for principal in accounts.chain(members.cloned()) {
        if !choices.iter().any(|choice| choice.principal == principal) {
            choices.push(MentionChoice {
                label: names.mention(&principal)[1..].to_string(),
                principal,
            });
        }
    }
    choices.sort_by_key(|choice| choice.label.to_lowercase());
    choices
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MentionChoice {
    pub label: String,
    pub principal: Principal,
}

/// The canonical token the composer inserts: `<@account>`.
pub fn mention_token(principal: &Principal) -> String {
    match principal {
        Principal::Account(account) => format!("<@{account}>"),
        Principal::Module(_) | Principal::Root => String::new(),
    }
}

/// The other account of a dm room `mine` is in.
pub fn dm_peer_of(mine: u64, channel_id: &str) -> Option<u64> {
    let (a, b) = chat::dm_peers(channel_id)?;
    (mine == a).then_some(b).or((mine == b).then_some(a))
}
