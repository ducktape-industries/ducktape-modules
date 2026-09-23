use super::*;

#[test]
fn admission_seats_members_and_the_next_epoch_reads_them() {
    deterministic::Runner::default().start(|context| async move {
        let dir = tempfile::tempdir().unwrap();
        let mut net = Net::found_with_puppet_admission(context, dir.path()).await;
        let stranger = net
            .refuse(
                &public(1),
                valset::PROGRAM,
                &valset::Op::Set(membership(3, valset::Standing::Resident)),
            )
            .await;
        assert_eq!(stranger, reason::UNAUTHORIZED);
        let other_program = net
            .sent_by(
                "probe",
                valset::PROGRAM,
                &valset::Op::Set(membership(3, valset::Standing::Resident)),
            )
            .await;
        assert_eq!(refusal_of(&other_program), reason::UNAUTHORIZED);
        let admitted = net
            .as_admission(&valset::Op::Set(membership(3, valset::Standing::Resident)))
            .await;
        output_of(&admitted);
        assert_eq!(net.memberships().await.len(), 3);
        assert_eq!(net.validators().await.len(), 2);
        let valset::Reply::Members(members) =
            net.ask(valset::PROGRAM, &valset::Query::Members).await
        else {
            panic!()
        };
        assert_eq!(members.len(), 3);
        let promoted = net
            .as_admission(&valset::Op::Set(membership(3, valset::Standing::Validator)))
            .await;
        output_of(&promoted);
        assert_eq!(net.validators().await.len(), 3);
        let epoch = (net.height + EPOCH_LENGTH) / EPOCH_LENGTH;
        while net.host.epoch_seating(epoch).unwrap().is_none() {
            net.tick().await;
        }
        let seating = net.host.epoch_seating(epoch).unwrap().unwrap();
        assert_eq!(seating.validators.len(), 3);
        assert_eq!(seating.members.len(), 3);
        for seed in [1, 2] {
            let removed = net
                .as_admission(&valset::Op::Remove { key: public(seed) })
                .await;
            output_of(&removed);
        }
        assert_eq!(net.validators().await, vec![public(3)]);
        let last = net
            .as_admission(&valset::Op::Remove { key: public(3) })
            .await;
        assert_eq!(refusal_of(&last), reason::WRONG_STATE);
        let demoted = net
            .as_admission(&valset::Op::Set(membership(3, valset::Standing::Resident)))
            .await;
        assert_eq!(refusal_of(&demoted), reason::WRONG_STATE);
        let valset::Reply::Membership(Some(standing)) = net
            .ask(
                valset::PROGRAM,
                &valset::Query::Membership { key: public(3) },
            )
            .await
        else {
            panic!()
        };
        assert_eq!(standing.standing, valset::Standing::Validator);
    });
}

#[test]
fn memberships_resume_in_key_order_at_the_answering_height() {
    deterministic::Runner::default().start(|context| async move {
        let dir = tempfile::tempdir().unwrap();
        let mut net = Net::found(context, dir.path()).await;
        net.tick().await;
        let mut after = None;
        let mut keys = Vec::new();
        loop {
            let valset::Reply::Memberships(reply) = net
                .ask(
                    valset::PROGRAM,
                    &valset::Query::Memberships {
                        page: Page {
                            after,
                            limit: Some(1),
                        },
                    },
                )
                .await
            else {
                panic!()
            };
            assert_eq!(reply.height, net.height);
            assert_eq!(reply.items.len(), 1);
            keys.push(reply.items[0].key.clone());
            after = reply.next;
            if after.is_none() {
                break;
            }
        }
        let mut expected = vec![public(1), public(2)];
        expected.sort();
        assert_eq!(keys, expected);
    });
}
