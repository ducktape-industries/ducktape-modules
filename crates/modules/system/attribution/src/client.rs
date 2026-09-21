//! Attribution for a source module: report an object's relation set in the
//! same unit as the write, as a function over [`Ctx`]. A caller names the
//! attribution module by id and never touches the bytes.
use sdk::{Ctx, Msg};

use crate::{Actor, AttributionMsg, ObjectRef, Relation, Transfer, encode_msg};

/// Report `object`'s FULL relation set at `revision`, as `actor`, in this
/// unit: the write and its attribution commit or abort together. An empty
/// set at a new revision is a delete.
pub fn attribute(
    ctx: &mut dyn Ctx,
    attribution: &str,
    object: ObjectRef,
    revision: u64,
    actor: Actor,
    relations: Vec<Relation>,
    transfers: Vec<Transfer>,
) {
    ctx.emit_msg(Msg {
        target: attribution.to_string(),
        payload: encode_msg(&AttributionMsg::Attribute {
            object,
            revision,
            actor,
            relations,
            transfers,
        }),
    });
}
