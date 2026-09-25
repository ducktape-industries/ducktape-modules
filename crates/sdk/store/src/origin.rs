//! The origin guards every module checks, on [`Env`].

use crate::{Env, Error, ModuleId, Origin, unauthorized};

impl Env {
    /// The signer's key, when a signed transaction made this call.
    pub fn signer(&self) -> Result<Vec<u8>, Error> {
        match &self.origin {
            Origin::Signed(key) => Ok(key.clone()),
            other => Err(unauthorized(format!(
                "only a signed transaction may do this, not {other:?}"
            ))),
        }
    }

    /// The module that sent this call, when a module did.
    pub fn sender_module(&self) -> Result<ModuleId, Error> {
        match &self.origin {
            Origin::Module(module) => Ok(module.clone()),
            other => Err(unauthorized(format!(
                "only a module may do this, not {other:?}"
            ))),
        }
    }

    /// Refused unless `module` (or the chain itself, [`Origin::Root`]) made
    /// this call.
    pub fn require_module(&self, module: &str) -> Result<(), Error> {
        let authorized = match &self.origin {
            Origin::Module(sender) => sender == module,
            Origin::Root => true,
            Origin::Signed(_) => false,
        };
        if !authorized {
            return Err(unauthorized(format!(
                "only {module} may do this, not {:?}",
                self.origin
            )));
        }
        Ok(())
    }
}
