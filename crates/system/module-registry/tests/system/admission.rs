use super::*;

fn enroll_op(seed: u64, invite: Option<admission::Invite>) -> admission::Op {
    admission::Op::Enroll {
        address: format!("node-{seed}:9000"),
        invite,
    }
}

fn counted(votes: u64, needed: u64) -> admission::Voted {
    admission::Voted::Counted { votes, needed }
}

#[test]
fn an_invite_signed_by_a_validator_seats_a_resident_once() {
    deterministic::Runner::default().start(|context| async move {
        let dir = tempfile::tempdir().unwrap();
        let mut net = Net::found(context, dir.path()).await;
        let other_program = net
            .sent_by(
                "probe",
                valset::PROGRAM,
                &valset::Op::Set(membership(3, valset::Standing::Resident)),
            )
            .await;
        assert_eq!(refusal_of(&other_program), reason::UNAUTHORIZED);
        let uninvited = net
            .refuse(&public(3), admission::PROGRAM, &enroll_op(3, None))
            .await;
        assert_eq!(uninvited, reason::UNAUTHORIZED);

        let invite = net.invite(1, 1);
        output_of(&net.enroll(3, "node-3:9000", Some(invite.clone())).await);
        let memberships = net.memberships().await;
        let joiner = memberships.iter().find(|m| m.key == public(3)).unwrap();
        assert_eq!(joiner.standing, valset::Standing::Resident);
        assert_eq!(joiner.address, "node-3:9000");
        let reused = net
            .refuse(&public(4), admission::PROGRAM, &enroll_op(4, Some(invite)))
            .await;
        assert_eq!(reused, reason::WRONG_STATE);
        let mut forged = net.invite(1, 2);
        forged.signature = net.invite(9, 2).signature;
        let forged = net
            .refuse(&public(4), admission::PROGRAM, &enroll_op(4, Some(forged)))
            .await;
        assert_eq!(forged, reason::UNAUTHORIZED);

        output_of(&net.enroll(1, "node-1:9001", None).await);
        let memberships = net.memberships().await;
        let validator = memberships.iter().find(|m| m.key == public(1)).unwrap();
        assert_eq!(validator.standing, valset::Standing::Validator);
        assert_eq!(validator.address, "node-1:9001");

        let epoch = (net.height + EPOCH_LENGTH) / EPOCH_LENGTH;
        while net.host.epoch_seating(epoch).unwrap().is_none() {
            net.tick().await;
        }
        let seating = net.host.epoch_seating(epoch).unwrap().unwrap();
        assert_eq!(seating.validators.len(), 2);
        assert_eq!(seating.members.len(), 3);
    });
}

#[test]
fn validators_vote_a_resident_up_down_and_out_and_the_door_open() {
    deterministic::Runner::default().start(|context| async move {
        let dir = tempfile::tempdir().unwrap();
        let mut net = Net::found(context, dir.path()).await;
        output_of(&net.enroll(3, "node-3:9000", Some(net.invite(1, 1))).await);

        let promote = admission::Motion::Promote { key: public(3) };
        assert_eq!(net.vote(1, promote.clone()).await, counted(1, 2));
        assert_eq!(net.vote(2, promote).await, admission::Voted::Enacted);
        assert_eq!(net.validators().await.len(), 3);
        let epoch = (net.height + EPOCH_LENGTH) / EPOCH_LENGTH;
        while net.host.epoch_seating(epoch).unwrap().is_none() {
            net.tick().await;
        }
        let seating = net.host.epoch_seating(epoch).unwrap().unwrap();
        assert!(seating.validators.contains(&public(3)));

        let demote = admission::Motion::Demote { key: public(3) };
        assert_eq!(net.vote(1, demote.clone()).await, counted(1, 3));
        assert_eq!(net.vote(2, demote.clone()).await, counted(2, 3));
        assert_eq!(net.vote(3, demote).await, admission::Voted::Enacted);
        assert_eq!(net.validators().await.len(), 2);

        let remove = admission::Motion::Remove { key: public(3) };
        net.vote(1, remove.clone()).await;
        assert_eq!(net.vote(2, remove).await, admission::Voted::Enacted);
        assert_eq!(net.memberships().await.len(), 2);

        let open = admission::Motion::Door { open: true };
        net.vote(1, open.clone()).await;
        assert_eq!(net.vote(2, open).await, admission::Voted::Enacted);
        output_of(&net.enroll(4, "node-4:9000", None).await);
        net.apply(&public(4), admission::PROGRAM, &admission::Op::Leave)
            .await;
        output_of(&net.delivered_to_valset().await);
        assert_eq!(net.memberships().await.len(), 2);
    });
}

#[test]
fn the_valset_holds_at_most_max_members() {
    deterministic::Runner::default().start(|context| async move {
        let dir = tempfile::tempdir().unwrap();
        let mut net = Net::found(context, dir.path()).await;
        let open = admission::Motion::Door { open: true };
        net.vote(1, open.clone()).await;
        net.vote(2, open).await;
        let joiners = 100..100 + (valset::MAX_MEMBERS as u64 - 2) + 1;
        let submissions = joiners
            .map(|seed| {
                let payload = abi::encode(&enroll_op(seed, None));
                net.submission(&public(seed), admission::PROGRAM, payload)
            })
            .collect();
        let applied = net.block(submissions).await;
        let admitted = applied
            .submissions
            .iter()
            .all(|receipt| matches!(receipt.outcome, Outcome::Applied { .. }));
        assert!(admitted);
        let applied = net.tick().await;
        let refused: Vec<&str> = applied
            .deliveries
            .iter()
            .filter_map(|delivered| match &delivered.receipt.outcome {
                Outcome::Rejected(refusal) => Some(refusal.reason.as_str()),
                Outcome::Applied { .. } => None,
            })
            .collect();
        assert_eq!(refused, vec![reason::CAPACITY]);
        let full = net
            .refuse(&public(5000), admission::PROGRAM, &enroll_op(5000, None))
            .await;
        assert_eq!(full, reason::CAPACITY);
    });
}
