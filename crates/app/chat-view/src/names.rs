//! What the network calls a principal, as the composer offers and inserts it.
//! The roster itself is [`chat::view::Names`], shared with every view that
//! names chat principals.
use chat::Principal;
use chat::view::Names;

/// Autocomplete candidates: every person and agent that acts
/// ([`Names::people`]), then the room's other such members, labelled
/// without the `@`.
pub fn mention_choices(names: &Names, members: &[Principal]) -> Vec<MentionChoice> {
    let accounts = names.people().map(Principal::Account);
    let members = members.iter().filter(|principal| names.pickable(principal));
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
        Principal::Root => String::new(),
    }
}

/// The other account of a dm room `mine` is in.
pub fn dm_peer_of(mine: u64, channel_id: &str) -> Option<u64> {
    let (a, b) = chat::dm_peers(channel_id)?;
    (mine == a).then_some(b).or((mine == b).then_some(a))
}

#[cfg(test)]
mod tests {
    use chat::{Category, Principal, Profile, Status};

    use super::*;

    fn profile(number: u64, name: &str) -> Profile {
        Profile {
            number,
            name: name.into(),
            category: None,
            manager: None,
            module: None,
            status: Status::Active,
        }
    }

    /// Mentions and reviewers offer people and agents that act: no
    /// module's account, no suspended or revoked one.
    #[test]
    fn the_pickers_offer_people_and_live_agents_only() {
        let names = Names::from_roster([
            profile(1, "ada"),
            Profile {
                category: Some(Category::Agent),
                manager: Some(1),
                ..profile(2, "scout")
            },
            Profile {
                module: Some("forge".into()),
                ..profile(3, "forge")
            },
            Profile {
                category: Some(Category::Agent),
                manager: Some(1),
                status: Status::Suspended,
                ..profile(4, "idle")
            },
            Profile {
                status: Status::Revoked,
                ..profile(5, "gone")
            },
        ]);
        assert_eq!(names.people().collect::<Vec<_>>(), [1, 2]);
        let members = [3, 4, 9].map(Principal::Account);
        let labels: Vec<_> = mention_choices(&names, &members)
            .into_iter()
            .map(|choice| choice.label)
            .collect();
        assert_eq!(labels, ["account-9", "ada", "scout"]);
        assert_eq!(names.member(&Principal::Account(4)), "idle (suspended)");
    }
}
