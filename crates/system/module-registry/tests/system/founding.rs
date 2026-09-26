use super::*;

#[test]
fn founding_seats_the_validators_and_every_program_answers() {
    deterministic::Runner::default().start(|context| async move {
        let dir = tempfile::tempdir().unwrap();
        let net = Net::found(context, dir.path()).await;
        let programs = net.host.programs().unwrap();
        for program in [
            module_registry::MODULE,
            valset::MODULE,
            identity::MODULE,
            AUTHORITY,
        ] {
            assert!(programs.contains_key(program), "{program} is not rostered");
        }
        assert!(!programs.contains_key("lens"), "a view is never admitted");
        let module_registry::Reply::Views(views) = net
            .ask(module_registry::MODULE, &module_registry::Query::Views(0))
            .await
        else {
            panic!()
        };
        let names: Vec<&str> = views.iter().map(|view| view.name.as_str()).collect();
        assert_eq!(names, ["lens"]);
        assert_eq!(
            net.host.blob(&views[0].view).unwrap().as_deref(),
            Some(&b"program 6\0a view"[..]),
            "the founding stored the view's bytes"
        );
        let memberships = net.memberships().await;
        assert_eq!(memberships.len(), 2);
        assert!(
            memberships
                .iter()
                .all(|membership| membership.role == valset::Role::Validator)
        );
        let seated = net.host.epoch_members(0).unwrap().unwrap();
        assert_eq!(seated.len(), 2);
        assert!(seated.contains(&member(1)));
        assert!(seated.contains(&member(2)));
        let identity::Reply::Accounts(accounts) = net
            .ask(
                identity::MODULE,
                &identity::Query::List {
                    page: PageRequest::default(),
                },
            )
            .await
        else {
            panic!()
        };
        assert_eq!(accounts.height, net.height);
        assert_eq!(accounts.next, None);
        // every founding program, and only they: each has its account
        let modules: Vec<_> = accounts
            .items
            .iter()
            .map(|account| account.module.as_deref())
            .collect();
        assert_eq!(
            modules,
            [
                Some(module_registry::MODULE),
                Some(valset::MODULE),
                Some(identity::MODULE),
                Some(AUTHORITY),
                Some("probe"),
            ]
        );
    });
}
