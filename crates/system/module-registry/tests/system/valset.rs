use super::*;

#[test]
fn the_authority_seats_members_and_the_next_epoch_reads_them() {
    deterministic::Runner::default().start(|context| async move {
        let dir = tempfile::tempdir().unwrap();
        let mut net = Net::found(context, dir.path()).await;
        let stranger = net
            .refuse(
                &public(1),
                valset::MODULE,
                &valset::Op::Set(membership(3, valset::Role::Resident)),
            )
            .await;
        assert_eq!(stranger, reason::UNAUTHORIZED);
        let other_program = net
            .sent_by(
                "probe",
                valset::MODULE,
                &valset::Op::Set(membership(3, valset::Role::Resident)),
            )
            .await;
        assert_eq!(refusal_of(&other_program), reason::UNAUTHORIZED);
        let admitted = net
            .as_authority(
                valset::MODULE,
                &valset::Op::Set(membership(3, valset::Role::Resident)),
            )
            .await;
        output_of(&admitted);
        assert_eq!(net.memberships().await.len(), 3);
        assert_eq!(net.validators().await.len(), 2);
        let valset::Reply::Members(members) =
            net.ask(valset::MODULE, &valset::Query::Members).await
        else {
            panic!()
        };
        assert_eq!(members.len(), 3);
        let promoted = net
            .as_authority(
                valset::MODULE,
                &valset::Op::Set(membership(3, valset::Role::Validator)),
            )
            .await;
        output_of(&promoted);
        assert_eq!(net.validators().await.len(), 3);
        let epoch = (net.height + EPOCH_LENGTH) / EPOCH_LENGTH;
        while net.host.epoch_members(epoch).unwrap().is_none() {
            net.tick().await;
        }
        assert_eq!(net.host.epoch_members(epoch).unwrap().unwrap().len(), 3);
        for seed in [1, 2] {
            let removed = net
                .as_authority(valset::MODULE, &valset::Op::Remove { key: public(seed) })
                .await;
            output_of(&removed);
        }
        assert_eq!(net.validators().await, vec![public(3)]);
        let last = net
            .as_authority(valset::MODULE, &valset::Op::Remove { key: public(3) })
            .await;
        assert_eq!(refusal_of(&last), reason::WRONG_STATE);
        let demoted = net
            .as_authority(
                valset::MODULE,
                &valset::Op::Set(membership(3, valset::Role::Resident)),
            )
            .await;
        assert_eq!(refusal_of(&demoted), reason::WRONG_STATE);
        let valset::Reply::Membership(Some(membership)) = net
            .ask(
                valset::MODULE,
                &valset::Query::Membership { key: public(3) },
            )
            .await
        else {
            panic!()
        };
        assert_eq!(membership.role, valset::Role::Validator);
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
                    valset::MODULE,
                    &valset::Query::Memberships {
                        page: PageRequest {
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
