//! The registry role: which code runs at a height. [`run`] checks every
//! rule below.

use abi::role::registry::{Entry, Genesis, Query, Reply, View};
use borsh::{BorshDeserialize, BorshSerialize};
use guest::{BlobId, Error, MockHost, Module, ModuleId};

use crate::{ask, init, same_bytes};

/// What the suite needs from the module beyond the role: publishing code
/// and scheduling changes, which the role leaves to it. Each op is sent at
/// `height`; a change lands at `at`.
pub trait Fixture {
    type Module: Module<Query: BorshSerialize, Response: BorshDeserialize>;

    /// A host before founding; the suite runs the module's init.
    fn host(&self) -> MockHost {
        MockHost::default()
    }

    /// Stores `body` as code; its blob id.
    fn publish(&self, host: &MockHost, height: u64, body: &[u8]) -> Result<BlobId, Error>;

    /// Schedules `entry` to run from `at`.
    fn set(&self, host: &MockHost, height: u64, at: u64, entry: Entry) -> Result<(), Error>;

    /// Schedules `program` to stop at `at`.
    fn remove(&self, host: &MockHost, height: u64, at: u64, program: &str) -> Result<(), Error>;

    /// The views the module lists at `height`, apart from its programs.
    fn views(&self, host: &MockHost, height: u64) -> Vec<View>;
}

/// Every registry rule, each on a fresh host.
pub fn run(fixture: &impl Fixture) {
    the_role_is_the_first_variants(fixture);
    genesis_programs_run_and_views_are_apart(fixture);
    a_set_lands_at_its_height_and_not_before(fixture);
    a_remove_drops_at_its_height(fixture);
}

fn module() -> String {
    MockHost::roles().registry
}

fn entry(program: &str, code: BlobId) -> Entry {
    Entry {
        program: program.into(),
        code,
        params: vec![],
    }
}

fn lens() -> View {
    View {
        name: "lens".into(),
        view: BlobId::Sha256([9; 32]),
    }
}

/// A host founded with programs `a` and `b` and the view `lens`.
fn founded<F: Fixture>(fixture: &F) -> MockHost {
    let host = fixture.host();
    let genesis = Genesis {
        programs: vec![
            entry("b", BlobId::Sha256([2; 32])),
            entry("a", BlobId::Sha256([1; 32])),
        ],
        views: vec![lens()],
    };
    init::<F::Module>(&host, &module(), &genesis);
    host
}

/// What `At(height)` answers, by program.
fn at<F: Fixture>(host: &MockHost, height: u64) -> Vec<Entry> {
    let asked = Query::At(height);
    let mut entries = match ask::<F::Module, Reply>(host, &module(), height, &asked) {
        Ok(Reply::Programs(entries)) => entries,
        other => panic!("registry: At({height}) answers Programs, not {other:?}"),
    };
    entries.sort_by(|a, b| a.program.cmp(&b.program));
    entries
}

fn programs<F: Fixture>(host: &MockHost, height: u64) -> Vec<ModuleId> {
    at::<F>(host, height)
        .into_iter()
        .map(|e| e.program)
        .collect()
}

/// The role's query and reply are the module's first variants, byte for
/// byte.
pub fn the_role_is_the_first_variants<F: Fixture>(_: &F) {
    type M<F> = <F as Fixture>::Module;
    let m = module();
    same_bytes::<<M<F> as Module>::Query>(&m, "Query", &Query::At(5));
    let reply = Reply::Programs(vec![entry("a", BlobId::Sha256([1; 32]))]);
    same_bytes::<<M<F> as Module>::Response>(&m, "Response", &reply);
}

/// `At` answers every founding program as founded, and no view: views are
/// listed apart.
pub fn genesis_programs_run_and_views_are_apart<F: Fixture>(fixture: &F) {
    let host = founded(fixture);
    assert_eq!(
        at::<F>(&host, 1),
        [
            entry("a", BlobId::Sha256([1; 32])),
            entry("b", BlobId::Sha256([2; 32]))
        ],
        "registry: At answers the founding programs, and no view"
    );
    assert_eq!(
        fixture.views(&host, 1),
        [lens()],
        "registry: the founding view is listed apart"
    );
}

/// Published code scheduled for a height runs from that height, not
/// before: a new program is admitted, a running one's code swapped.
pub fn a_set_lands_at_its_height_and_not_before<F: Fixture>(fixture: &F) {
    let host = founded(fixture);
    let code = fixture
        .publish(&host, 1, b"new code")
        .unwrap_or_else(|e| panic!("registry: publishing code: {e:?}"));
    for program in ["new", "a"] {
        fixture
            .set(&host, 1, 5, entry(program, code))
            .unwrap_or_else(|e| panic!("registry: scheduling {program} at 5: {e:?}"));
    }
    assert_eq!(
        at::<F>(&host, 4),
        at::<F>(&host, 1),
        "registry: nothing scheduled at 5 runs at 4"
    );
    for height in [5, 6] {
        assert_eq!(
            at::<F>(&host, height),
            [
                entry("a", code),
                entry("b", BlobId::Sha256([2; 32])),
                entry("new", code)
            ],
            "registry: at {height}, the set scheduled at 5 runs"
        );
    }
}

/// A program scheduled to stop at a height runs until it, and not from it,
/// even once the module has run past it.
pub fn a_remove_drops_at_its_height<F: Fixture>(fixture: &F) {
    let host = founded(fixture);
    fixture
        .remove(&host, 1, 6, "b")
        .unwrap_or_else(|e| panic!("registry: scheduling b's removal at 6: {e:?}"));
    assert_eq!(
        programs::<F>(&host, 5),
        ["a", "b"],
        "registry: b runs until 6"
    );
    assert_eq!(
        programs::<F>(&host, 6),
        ["a"],
        "registry: b is dropped at 6"
    );
    fixture
        .publish(&host, 7, b"later")
        .unwrap_or_else(|e| panic!("registry: publishing at 7: {e:?}"));
    assert_eq!(
        programs::<F>(&host, 7),
        ["a"],
        "registry: b stays dropped once the module has run past 6"
    );
    assert_eq!(
        fixture.views(&host, 7),
        [lens()],
        "registry: removing a program leaves the views"
    );
}
