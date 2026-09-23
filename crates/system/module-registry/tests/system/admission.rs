use super::*;

#[test]
fn a_signed_enroll_seats_a_resident_and_a_validator_keeps_its_standing() {
    deterministic::Runner::default().start(|context| async move {
        let dir = tempfile::tempdir().unwrap();
        let mut net = Net::found(context, dir.path()).await;
        // Nobody but admission writes the valset.
        let other_program = net
            .sent_by(
                "probe",
                valset::PROGRAM,
                &valset::Op::Set(membership(3, valset::Standing::Resident)),
            )
            .await;
        assert_eq!(refusal_of(&other_program), reason::UNAUTHORIZED);
        // A stranger enrolls itself as a resident.
        let seated = net.enroll(3, "node-3:9000").await;
        output_of(&seated);
        let memberships = net.memberships().await;
        assert_eq!(memberships.len(), 3);
        let joiner = memberships.iter().find(|m| m.key == public(3)).unwrap();
        assert_eq!(joiner.standing, valset::Standing::Resident);
        assert_eq!(joiner.address, "node-3:9000");
        assert_eq!(net.validators().await.len(), 2);
        // A validator re-enrolling moves its address and keeps its seat.
        let moved = net.enroll(1, "node-1:9001").await;
        output_of(&moved);
        let memberships = net.memberships().await;
        let validator = memberships.iter().find(|m| m.key == public(1)).unwrap();
        assert_eq!(validator.standing, valset::Standing::Validator);
        assert_eq!(validator.address, "node-1:9001");
        assert_eq!(net.validators().await.len(), 2);
        // The next epoch reads all three members.
        let epoch = (net.height + EPOCH_LENGTH) / EPOCH_LENGTH;
        while net.host.epoch_members(epoch).unwrap().is_none() {
            net.tick().await;
        }
        assert_eq!(net.host.epoch_members(epoch).unwrap().unwrap().len(), 3);
    });
}
