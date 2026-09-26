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
    use chat::{Category, Kind, Principal, Profile, Standing};

    use super::*;

    fn profile(number: u64, name: &str, kind: Kind) -> Profile {
        Profile {
            number,
            name: name.into(),
            kind,
        }
    }

    fn agent(manager: u64, standing: Standing) -> Kind {
        Kind::Managed {
            manager,
            category: Category::Agent,
            standing,
        }
    }

    /// Mentions and reviewers offer people and agents that act: no
    /// module's account, no suspended or revoked one. Each wears the one
    /// badge and note its kind gives it.
    #[test]
    fn the_pickers_offer_people_and_live_agents_only() {
        let names = Names::from_roster([
            profile(1, "ada", Kind::Person),
            profile(2, "scout", agent(1, Standing::Active)),
            profile(3, "forge", Kind::Module("forge".into())),
            profile(4, "idle", agent(1, Standing::Suspended)),
            profile(5, "gone", agent(1, Standing::Revoked)),
        ]);
        assert_eq!(names.people().collect::<Vec<_>>(), [1, 2]);
        let members = [3, 4, 9].map(Principal::Account);
        let labels: Vec<_> = mention_choices(&names, &members)
            .into_iter()
            .map(|choice| choice.label)
            .collect();
        assert_eq!(labels, ["account-9", "ada", "scout"]);
        let account = Principal::Account;
        assert_eq!(names.member(&account(4)), "idle (suspended)");
        assert_eq!(names.member(&account(5)), "gone (revoked)");
        assert_eq!(names.badge(&account(1)), None);
        assert_eq!(
            names.badge(&account(2)).as_deref(),
            Some("Agent · managed by ada")
        );
        assert_eq!(names.badge(&account(3)).as_deref(), Some("Module · forge"));
        assert_eq!(names.module(&account(3)), Some("forge"));
        assert!(crate::message::agent(&names, &account(2)));
        assert!(!crate::message::agent(&names, &account(3)));
    }
}
