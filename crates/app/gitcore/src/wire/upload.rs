// upload-pack protocol v2: capability advertisement, ls-refs and fetch command parsing, and the fetch response.

use super::{pktline, sideband};
use crate::error::{Error, Result};
use crate::object::{Kind, Tag};
use crate::oid::{Hash, Oid};
use crate::pack::PackWriter;
use crate::store::{load, Objects};
use crate::walk::{
    commit_of, commits, is_ancestor, reachable_from_trees, reachable_objects, Verdict,
};
use alloc::collections::{BTreeMap, BTreeSet};
use alloc::vec::Vec;

pub fn capability_advertisement(hash: Hash, agent: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    pktline::push_line(&mut out, b"version 2");
    let mut agent_line = b"agent=".to_vec();
    agent_line.extend_from_slice(agent);
    pktline::push_line(&mut out, &agent_line);
    pktline::push_line(&mut out, b"ls-refs=unborn");
    pktline::push_line(&mut out, b"fetch=wait-for-done");
    pktline::push_line(&mut out, b"server-option");
    let mut format_line = b"object-format=".to_vec();
    format_line.extend_from_slice(hash.name().as_bytes());
    pktline::push_line(&mut out, &format_line);
    out.extend_from_slice(pktline::flush());
    out
}

#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct LsRefs {
    pub symrefs: bool,
    pub peel: bool,
    pub unborn: bool,
    pub ref_prefixes: Vec<Vec<u8>>,
}

#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Fetch {
    pub wants: Vec<Oid>,
    pub haves: Vec<Oid>,
    pub done: bool,
    pub thin_pack: bool,
    pub no_progress: bool,
    pub include_tag: bool,
    pub ofs_delta: bool,
    pub sideband_all: bool,
    pub wait_for_done: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Command {
    LsRefs(LsRefs),
    Fetch(Fetch),
}

pub fn parse_command(bytes: &[u8], hash: Hash) -> Result<Option<Command>> {
    let mut reader = pktline::Reader::new(bytes);
    let mut command = None;
    let mut saw_key = false;
    let mut saw_delim = false;
    for pkt in reader.by_ref() {
        match pkt? {
            pktline::Pkt::Data(data) => {
                saw_key = true;
                let line = pktline::strip_newline(data);
                if let Some(name) = line.strip_prefix(b"command=") {
                    command = Some(name.to_vec());
                    continue;
                }
                if let Some(format) = line.strip_prefix(b"object-format=") {
                    check_format(format, hash)?;
                    continue;
                }
                let known_prefix =
                    line.starts_with(b"agent=") || line.starts_with(b"server-option=");
                if !known_prefix {
                    return Err(Error::BadRequest);
                }
            }
            pktline::Pkt::Delim => {
                saw_delim = true;
                break;
            }
            pktline::Pkt::Flush => break,
            pktline::Pkt::ResponseEnd => return Err(Error::BadRequest),
        }
    }
    let mut args: Vec<&[u8]> = Vec::new();
    if saw_delim {
        while let Some(line) = pktline::read_data(&mut reader)? {
            args.push(line);
        }
    }
    let session_ended = !saw_key;
    if session_ended {
        return Ok(None);
    }
    match command.as_deref() {
        Some(b"ls-refs") => Ok(Some(Command::LsRefs(parse_ls_refs(&args)?))),
        Some(b"fetch") => Ok(Some(Command::Fetch(parse_fetch(&args, hash)?))),
        Some(_) => Err(Error::UnknownCommand),
        None => Err(Error::BadRequest),
    }
}

fn check_format(format: &[u8], hash: Hash) -> Result<()> {
    let Some(actual) = Hash::from_name(format) else {
        return Err(Error::Unsupported);
    };
    let matches = actual == hash;
    if !matches {
        return Err(Error::WrongHash {
            expected: hash,
            actual,
        });
    }
    Ok(())
}

fn parse_ls_refs(args: &[&[u8]]) -> Result<LsRefs> {
    let mut command = LsRefs::default();
    for arg in args {
        match *arg {
            b"symrefs" => command.symrefs = true,
            b"peel" => command.peel = true,
            b"unborn" => command.unborn = true,
            other => {
                let Some(prefix) = other.strip_prefix(b"ref-prefix ") else {
                    return Err(Error::BadRequest);
                };
                command.ref_prefixes.push(prefix.to_vec());
            }
        }
    }
    Ok(command)
}

fn parse_fetch(args: &[&[u8]], hash: Hash) -> Result<Fetch> {
    let mut command = Fetch::default();
    for arg in args {
        match *arg {
            b"done" => command.done = true,
            b"thin-pack" => command.thin_pack = true,
            b"no-progress" => command.no_progress = true,
            b"include-tag" => command.include_tag = true,
            b"ofs-delta" => command.ofs_delta = true,
            b"sideband-all" => command.sideband_all = true,
            b"wait-for-done" => command.wait_for_done = true,
            other => {
                if let Some(hex) = other.strip_prefix(b"want ") {
                    command.wants.push(Oid::from_hex(hash, hex)?);
                    continue;
                }
                if let Some(hex) = other.strip_prefix(b"have ") {
                    command.haves.push(Oid::from_hex(hash, hex)?);
                    continue;
                }
                let unsupported = other.starts_with(b"deepen")
                    || other.starts_with(b"shallow ")
                    || other.starts_with(b"filter ")
                    || other.starts_with(b"want-ref ")
                    || other.starts_with(b"packfile-uris");
                if unsupported {
                    return Err(Error::Unsupported);
                }
                return Err(Error::BadRequest);
            }
        }
    }
    Ok(command)
}

fn matches_prefix(name: &[u8], prefixes: &[Vec<u8>]) -> bool {
    prefixes.is_empty() || prefixes.iter().any(|prefix| name.starts_with(prefix))
}

fn peel_tag<S: Objects + ?Sized>(store: &S, id: &Oid, cap: usize) -> Result<Option<Oid>> {
    let mut current = *id;
    let mut peeled = false;
    for _ in 0..=cap {
        let Some(object) = store.get(&current)? else {
            return Err(Error::MissingObject(current));
        };
        let is_tag = object.kind == Kind::Tag;
        if !is_tag {
            return Ok(peeled.then_some(current));
        }
        current = Tag::parse(&object.body, current.hash())?.object;
        peeled = true;
    }
    Err(Error::CapReached)
}

pub fn ls_refs_response<S: Objects + ?Sized>(
    store: &S,
    refs: &BTreeMap<Vec<u8>, Oid>,
    head: Option<&[u8]>,
    command: &LsRefs,
    cap: usize,
) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    let head_wanted = head.is_some() && matches_prefix(b"HEAD", &command.ref_prefixes);
    if let (true, Some(target)) = (head_wanted, head) {
        let mut line = match refs.get(target) {
            Some(id) => id.to_hex().into_bytes(),
            None => b"unborn".to_vec(),
        };
        let unborn = !refs.contains_key(target);
        let listed = !unborn || command.unborn;
        if listed {
            line.extend_from_slice(b" HEAD");
            let show_target = command.symrefs || unborn;
            if show_target {
                line.extend_from_slice(b" symref-target:");
                line.extend_from_slice(target);
            }
            pktline::push_line(&mut out, &line);
        }
    }
    for (name, id) in refs {
        let wanted = matches_prefix(name, &command.ref_prefixes);
        if !wanted {
            continue;
        }
        let mut line = id.to_hex().into_bytes();
        line.push(b' ');
        line.extend_from_slice(name);
        if command.peel {
            if let Some(target) = peel_tag(store, id, cap)? {
                line.extend_from_slice(b" peeled:");
                line.extend_from_slice(target.to_hex().as_bytes());
            }
        }
        pktline::push_line(&mut out, &line);
    }
    out.extend_from_slice(pktline::flush());
    Ok(out)
}

pub fn acknowledgments(common: &[Oid], ready: bool) -> Vec<u8> {
    let mut out = Vec::new();
    pktline::push_line(&mut out, b"acknowledgments");
    if common.is_empty() {
        pktline::push_line(&mut out, b"NAK");
    }
    for id in common {
        let mut line = b"ACK ".to_vec();
        line.extend_from_slice(id.to_hex().as_bytes());
        pktline::push_line(&mut out, &line);
    }
    if ready {
        pktline::push_line(&mut out, b"ready");
    }
    out
}

pub fn packfile_header() -> Vec<u8> {
    pktline::encode(b"packfile\n")
}

struct Selection {
    tags: Vec<Oid>,
    tips: Vec<Oid>,
    loose_trees: BTreeSet<Oid>,
    loose_blobs: BTreeSet<Oid>,
}

fn select_wants<S: Objects + ?Sized>(
    store: &S,
    refs: &BTreeMap<Vec<u8>, Oid>,
    wants: &[Oid],
    cap: usize,
) -> Result<Selection> {
    let advertised: BTreeSet<&Oid> = refs.values().collect();
    let mut selection = Selection {
        tags: Vec::new(),
        tips: Vec::new(),
        loose_trees: BTreeSet::new(),
        loose_blobs: BTreeSet::new(),
    };
    for want in wants {
        let allowed = advertised.contains(want);
        if !allowed {
            return Err(Error::NotAdvertised(*want));
        }
        select_want(store, *want, cap, &mut selection)?;
    }
    Ok(selection)
}

fn select_want<S: Objects + ?Sized>(
    store: &S,
    want: Oid,
    cap: usize,
    selection: &mut Selection,
) -> Result<()> {
    let mut current = want;
    for _ in 0..=cap {
        let object = load(store, &current)?;
        match object.kind {
            Kind::Tag => {
                selection.tags.push(current);
                current = Tag::parse(&object.body, current.hash())?.object;
            }
            Kind::Commit => {
                selection.tips.push(current);
                return Ok(());
            }
            Kind::Tree => {
                selection.loose_trees.insert(current);
                return Ok(());
            }
            Kind::Blob => {
                selection.loose_blobs.insert(current);
                return Ok(());
            }
        }
    }
    Err(Error::CapReached)
}

fn all_wants_reachable<S: Objects + ?Sized>(
    store: &S,
    tips: &[Oid],
    common: &[Oid],
    cap: usize,
) -> Result<bool> {
    for tip in tips {
        let mut covered = false;
        for have in common {
            let verdict = is_ancestor(store, have, tip, cap)?;
            let reaches = verdict == Verdict::Yes;
            if reaches {
                covered = true;
                break;
            }
        }
        if !covered {
            return Ok(false);
        }
    }
    Ok(true)
}

pub fn fetch<S: Objects + ?Sized>(
    store: &S,
    refs: &BTreeMap<Vec<u8>, Oid>,
    command: &Fetch,
    cap: usize,
    sink: &mut dyn FnMut(&[u8]),
) -> Result<()> {
    let Some(first_want) = command.wants.first() else {
        return Err(Error::BadRequest);
    };
    let hash = first_want.hash();
    let selection = select_wants(store, refs, &command.wants, cap)?;
    let mut common = Vec::new();
    for have in &command.haves {
        let new_common = store.has(have)? && !common.contains(have);
        if new_common {
            common.push(*have);
        }
    }
    if !command.done {
        let ready =
            !command.wait_for_done && all_wants_reachable(store, &selection.tips, &common, cap)?;
        sink(&acknowledgments(&common, ready));
        if !ready {
            sink(pktline::flush());
            return Ok(());
        }
        sink(pktline::delim());
    }
    let new_commits = commits(store, &selection.tips, &common, cap)?;
    let new_set: BTreeSet<Oid> = new_commits.iter().copied().collect();
    let mut boundary: BTreeSet<Oid> = BTreeSet::new();
    for id in &new_commits {
        for parent in commit_of(store, id)?.parents {
            let outside = !new_set.contains(&parent);
            if outside {
                boundary.insert(parent);
            }
        }
    }
    for have in &common {
        let is_commit = store
            .get(have)?
            .is_some_and(|object| object.kind == Kind::Commit);
        if is_commit {
            boundary.insert(*have);
        }
    }
    let boundary: Vec<Oid> = boundary.into_iter().collect();
    let seen = reachable_objects(store, &boundary, &BTreeSet::new())?;
    let mut objects = reachable_objects(store, &new_commits, &seen)?;
    let loose_trees: Vec<Oid> = selection.loose_trees.iter().copied().collect();
    objects.extend(reachable_from_trees(store, &loose_trees, &seen)?);
    objects.extend(selection.loose_blobs.iter().copied());
    let mut order: Vec<Oid> = Vec::new();
    order.extend(selection.tags.iter().copied());
    order.extend(new_commits.iter().copied());
    order.extend(objects.iter().copied());
    let count = u32::try_from(order.len()).map_err(|_| Error::TooManyObjects)?;

    sink(&packfile_header());
    let mut banded = sideband::Writer::new(sideband::Band::Pack, sink);
    {
        let mut write = |bytes: &[u8]| banded.write(bytes);
        let mut writer = PackWriter::new(&mut write, hash, count);
        for id in &order {
            writer.add(&load(store, id)?);
        }
        writer.finish()?;
    }
    banded.finish();
    sink(pktline::flush());
    Ok(())
}
