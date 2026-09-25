//! A module written with `guest` alone: a counter anyone may add to.

use guest::{ExecCtx, Module, QueryCtx, Refusal, invalid};

pub struct Counter;

impl Module for Counter {
    /// How much to add.
    type Op = u64;
    type Query = ();
    /// The count.
    type Response = u64;

    fn execute(ctx: &ExecCtx, by: u64) -> Result<(), Refusal> {
        let n = count(ctx)?
            .checked_add(by)
            .ok_or_else(|| invalid("the count would overflow"))?;
        ctx.put("n", &n);
        ctx.output(abi_bytes(n));
        Ok(())
    }

    fn query(ctx: &QueryCtx, (): ()) -> Result<u64, Refusal> {
        count(ctx)
    }
}

fn count(ctx: &QueryCtx) -> Result<u64, Refusal> {
    Ok(ctx.record("n")?.unwrap_or(0))
}

fn abi_bytes(n: u64) -> Vec<u8> {
    guest::abi::encode(&n)
}

guest::export!(Counter);

#[cfg(test)]
mod tests {
    use guest::{Cause, Env, MockHost, Origin, reason};

    use super::*;

    fn env() -> Env {
        Env {
            network: vec![],
            height: 1,
            time: 0,
            me: "counter".into(),
            origin: Origin::External(vec![1; 32]),
            cause: Cause::Direct,
        }
    }

    #[test]
    fn adds_and_answers_and_a_refusal_writes_nothing() {
        let host = MockHost::default();
        Counter::execute(&host.exec(env()), 2).unwrap();
        Counter::execute(&host.exec(env()), 3).unwrap();
        assert_eq!(Counter::query(&host.query(env()), ()).unwrap(), 5);
        assert_eq!(host.take_output(), guest::abi::encode(&5u64));

        let overflow = host.refused(|| Counter::execute(&host.exec(env()), u64::MAX));
        assert_eq!(overflow.reason, reason::INVALID_INPUT);

        // The bytes path `export!`'s `call` takes: decode, run, respond.
        guest::execute::<Counter>(&host.exec(env()), &guest::abi::encode(&1u64)).unwrap();
        guest::query::<Counter>(&host.query(env()), &guest::abi::encode(&())).unwrap();
        assert_eq!(host.borrow().response, guest::abi::encode(&6u64));
        let garbage = guest::execute::<Counter>(&host.exec(env()), &[1]).unwrap_err();
        assert_eq!(garbage.reason, reason::INVALID_INPUT);
    }
}

// An example is a binary unless it has a `main`; as a cdylib it needs none,
// but `cargo test` builds it as a test harness, which brings its own.
