//! Who may call, read off the env's [`Origin`]: the checks a module makes
//! before it trusts a frame's raw caller rather than its sender.

use crate::{Env, Error, ModuleId, Origin, unauthorized};

impl Env {
    /// The signing key of a signed frame; any other origin is refused.
    pub fn signer(&self) -> Result<Vec<u8>, Error> {
        match &self.origin {
            Origin::Signed(key) => Ok(key.clone()),
            other => Err(unauthorized(format!(
                "only a signed frame may do this, not {other:?}"
            ))),
        }
    }

    /// The module that sent this message; any other origin is refused.
    pub fn sending_module(&self) -> Result<ModuleId, Error> {
        match &self.origin {
            Origin::Module(module) => Ok(module.clone()),
            other => Err(unauthorized(format!(
                "only a module may do this, not {other:?}"
            ))),
        }
    }

    /// Who may change the modules and the validators. A stub that admits
    /// every origin until the chain has an authority to ask; swapping it
    /// for the real check is this one function.
    pub fn authority(&self) -> Result<(), Error> {
        Ok(())
    }

    /// Refused unless `module` or the chain itself sent this.
    pub fn sent_by(&self, module: &str) -> Result<(), Error> {
        let by_module = matches!(&self.origin, Origin::Module(sender) if sender == module);
        if by_module || self.origin == Origin::Root {
            return Ok(());
        }
        Err(unauthorized(format!(
            "only {module} may do this, not {:?}",
            self.origin
        )))
    }
}

#[cfg(test)]
mod tests {
    use crate::{Cause, Env, Origin, code};

    fn env(origin: Origin) -> Env {
        Env {
            chain_id: Vec::new(),
            height: 0,
            time: 0,
            module: "m".into(),
            origin,
            sender: None,
            roles: crate::MockHost::roles(),
            cause: Cause::Direct,
        }
    }

    #[test]
    fn each_check_admits_only_its_origin() {
        let signed = env(Origin::Signed(vec![1]));
        let module = env(Origin::Module("gov".into()));
        let root = env(Origin::Root);
        assert_eq!(signed.signer().unwrap(), [1]);
        assert_eq!(module.signer().unwrap_err().code, code::UNAUTHORIZED);
        assert_eq!(module.sending_module().unwrap(), "gov");
        assert_eq!(root.sending_module().unwrap_err().code, code::UNAUTHORIZED);
        assert!(module.sent_by("gov").is_ok() && root.sent_by("gov").is_ok());
        assert!(module.sent_by("other").is_err() && signed.sent_by("gov").is_err());
    }
}
