//! What an account's keys signed lately. Nothing indexes an account's
//! history, so, as Explorer's account page does, this reads the node's
//! recent window of blocks, newest first, and keeps the transactions one of
//! the account's keys signed, each titled by its own program
//! (`module.describe`).
//!
//! The window read is copied from explorer-view (`WINDOW`, its page size,
//! `ago` and `date`) rather than linked: a view crate exports its own entry points
//! and cannot be a dependency of another.
use ducktape_view_guest::Host;
use ducktape_view_guest::design;
use ducktape_view_guest::host::Error;
use ducktape_view_guest::methods::{BlockPage, ChainBlocks, ModuleDescribe};
use serde::{Deserialize, Serialize};

/// The recent window, in blocks: explorer-view's `WINDOW`.
pub const WINDOW: u64 = 1_000;
/// Blocks per `chain.blocks` page: explorer-view's, small so one reply's
/// decoding stays well inside a tick's fuel.
const PAGE: u32 = 20;
/// The most transactions the detail lists; the scan stops once it has them.
pub const SHOWN: usize = 8;

/// The newest transactions an account signed in the window.
#[derive(Clone, Default, Serialize, Deserialize)]
pub struct Recent {
    /// the newest block's time, which `when` counts back from
    pub now: u64,
    pub items: Vec<Signed>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Signed {
    /// what its program says it does: `Post in #general`
    pub title: String,
    pub height: u64,
    pub time: u64,
}

pub async fn recent(host: Host, keys: Vec<Vec<u8>>) -> Result<Recent, Error> {
    let mut now = None;
    let mut found = Vec::new();
    let mut read = 0;
    let mut before = None;
    'window: loop {
        let page = host
            .ask::<ChainBlocks>(BlockPage {
                before,
                limit: PAGE,
            })
            .await?;
        let full = page.len() == PAGE as usize;
        for block in page {
            now.get_or_insert(block.time);
            read += 1;
            before = Some(block.height);
            for tx in block.txs.into_iter().rev() {
                if keys.contains(&tx.signer) {
                    found.push((block.height, block.time, tx.target, tx.payload));
                    if found.len() == SHOWN {
                        break 'window;
                    }
                }
            }
            if read == WINDOW || block.height == 0 {
                break 'window;
            }
        }
        if !full {
            break;
        }
    }
    let mut items = Vec::with_capacity(found.len());
    for (height, time, target, payload) in found {
        let described = host
            .ask::<ModuleDescribe>((target.clone(), payload.clone()))
            .await;
        let title = match described {
            Ok(Some(op)) => op.title,
            // a program that says nothing, or a refusal: its name and size
            Ok(None) | Err(_) => format!(
                "{target} · {}",
                design::plural(payload.len() as u64, "byte", "bytes")
            ),
        };
        items.push(Signed {
            title,
            height,
            time,
        });
    }
    Ok(Recent {
        now: now.unwrap_or(0),
        items,
    })
}

/// How long before `now` a time in milliseconds was: `2s`, `3m`, `4h`, `5d`.
pub fn ago(now: u64, then: u64) -> String {
    let seconds = now.saturating_sub(then) / 1000;
    match seconds {
        0..60 => format!("{seconds}s"),
        60..3_600 => format!("{}m", seconds / 60),
        3_600..86_400 => format!("{}h", seconds / 3_600),
        _ => format!("{}d", seconds / 86_400),
    }
}
