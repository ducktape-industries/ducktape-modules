use super::*;

#[test]
fn founding_seats_the_validators_and_every_program_answers() {
    if !built() {
        return;
    }
    deterministic::Runner::default().start(|context| async move {
        let dir = tempfile::tempdir().unwrap();
        let net = Net::found(context, dir.path()).await;
        let programs = net.host.programs().unwrap();
        for program in [
            module_registry::PROGRAM,
            valset::PROGRAM,
            identity::PROGRAM,
            AUTHORITY,
        ] {
            assert!(programs.contains_key(program), "{program} is not rostered");
        }
        let memberships = net.memberships().await;
        assert_eq!(memberships.len(), 2);
        assert!(
            memberships
                .iter()
                .all(|membership| membership.standing == valset::Standing::Validator)
        );
        let seated = net.host.epoch_members(0).unwrap().unwrap();
        assert_eq!(seated.len(), 2);
        assert!(seated.contains(&member(1)));
        assert!(seated.contains(&member(2)));
        let identity::Reply::Accounts(accounts) = net
            .ask(
                identity::PROGRAM,
                &identity::Query::List {
                    page: Page::default(),
                },
            )
            .await
        else {
            panic!()
        };
        assert_eq!(accounts.height, net.height);
        assert_eq!(accounts.next, None);
        assert!(accounts.items.is_empty());
    });
}
