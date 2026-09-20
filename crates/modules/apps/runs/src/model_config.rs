//! Model configuration belongs to runs; identity remains the account authority.
//! Any submitter may configure any model: the record confers no authority, so
//! there is nothing a configuration could escalate.
use super::*;
use capability::validate_tag;
use sdk::refusal;

const MODEL_INDEX_KEY: &str = "model/index";
const MODEL_ITEM_PREFIX: &str = "model/item/";
const MODEL_OWNER_PREFIX: &str = "model/owner/";

/// a skill's `source_prefix` must be a SCOPED duckfs subtree, never a
/// namespace root: `resolve_skills` copies it verbatim into a run's
/// dispatch payload, and the provisioner's checkout runs one full checkout
/// per mount, so an unscoped prefix ("/", "/shared", "/home/<x>") makes every
/// run of the agent check out the whole namespace once per curated skill.
///
/// this is a local, minimal stand-in for `files::paths::canonical` (absolute,
/// `/`-separated, no empty/dot segments, at least 3 segments deep — the same
/// depth `library_skills` requires for `/shared/skills/<name>`): the agent
/// module is self-contained by design (see the crate doc), so it does not
/// depend on another module's crate for this shape check.
fn is_scoped_duckfs_prefix(prefix: &str) -> bool {
    if !prefix.starts_with('/') || prefix.contains('\0') {
        return false;
    }
    let segments: Vec<&str> = prefix.trim_start_matches('/').split('/').collect();
    let all_named = segments
        .iter()
        .all(|s| !s.is_empty() && *s != "." && *s != "..");
    all_named && segments.len() >= 3
}

// ---- the module -----------------------------------------------------------

impl RunsModule {
    /// a recipe hash is empty (unset) or exactly [`RECIPE_HASH_LEN`] bytes.
    fn validate_recipe_hash(recipe_hash: &[u8]) -> Result<(), Error> {
        if !recipe_hash.is_empty() && recipe_hash.len() != RECIPE_HASH_LEN {
            return Err(Error::Module {
                reason: refusal::INVALID_INPUT.into(),
                sentence: format!(
                    "recipe_hash must be empty or {RECIPE_HASH_LEN} bytes, got {}",
                    recipe_hash.len()
                ),
            });
        }
        Ok(())
    }

    /// a skill ref must carry a name that is [`is_skill_mount_name`] (the
    /// SAME predicate the noded provisioner's `mount_dir_name` calls — one
    /// rule, not two that could drift), unique within the record, and a
    /// source_prefix that is a scoped duckfs subtree ([`is_scoped_duckfs_prefix`]),
    /// also unique within the record. a pinned snapshot, when present, must be
    /// non-empty. order is preserved verbatim (skills are an ordered override
    /// list).
    ///
    /// the COUNT is capped ([`MAX_SKILLS_PER_AGENT`]) for the same reason the
    /// record's bytes are: the list is replicated state, and it is also the run's
    /// context budget. curation is the whole point of the tier design — a
    /// 500-skill list is a library, and the library lives in duckfs, not in the
    /// record.
    fn validate_skills(skills: &[SkillRef]) -> Result<(), Error> {
        if skills.len() > MAX_SKILLS_PER_AGENT {
            return Err(Error::Module {
                reason: refusal::CAPACITY.into(),
                sentence: format!(
                    "an agent may curate at most {MAX_SKILLS_PER_AGENT} skills, got {}; leave the \
                 rest in the shared skill library",
                    skills.len()
                ),
            });
        }
        let mut names = BTreeSet::new();
        let mut prefixes = BTreeSet::new();
        for skill in skills {
            if !is_skill_mount_name(&skill.name) {
                return Err(Error::Module {
                    reason: refusal::INVALID_INPUT.into(),
                    sentence: format!(
                        "skill name {:?} is not a safe mount directory name (want \
                     [a-zA-Z0-9._-]+, at most {MAX_SKILL_NAME_BYTES} bytes, not \".\" or \"..\")",
                        skill.name
                    ),
                });
            }
            if !names.insert(skill.name.as_str()) {
                return Err(Error::Module {
                    reason: refusal::INVALID_INPUT.into(),
                    sentence: format!("duplicate skill name {:?}", skill.name),
                });
            }
            if !is_scoped_duckfs_prefix(&skill.source_prefix) {
                return Err(Error::Module {
                    reason: refusal::INVALID_INPUT.into(),
                    sentence: format!(
                        "skill source_prefix {:?} is not a scoped duckfs subtree \
                     (want an absolute path at least 3 segments deep, e.g. \
                     /shared/skills/<name>)",
                        skill.source_prefix
                    ),
                });
            }
            if !prefixes.insert(skill.source_prefix.as_str()) {
                return Err(Error::Module {
                    reason: refusal::INVALID_INPUT.into(),
                    sentence: format!("duplicate skill source_prefix {:?}", skill.source_prefix),
                });
            }
            if let Some(snapshot) = &skill.source_snapshot
                && snapshot.is_empty()
            {
                return Err(Error::Module {
                    reason: refusal::INVALID_INPUT.into(),
                    sentence: "skill source_snapshot must not be empty when set".into(),
                });
            }
        }
        Ok(())
    }

    fn model_item_key(id: &str) -> String {
        format!("{MODEL_ITEM_PREFIX}{id}")
    }

    pub(crate) fn model_owner_key(owner: &RunOrigin) -> String {
        format!(
            "{MODEL_OWNER_PREFIX}{}",
            serde_json::to_string(owner).expect("model owner serializes")
        )
    }

    fn corrupt_record(sentence: impl Into<String>) -> Error {
        Error::Module {
            reason: refusal::CORRUPT.into(),
            sentence: sentence.into(),
        }
    }

    fn validate_model_record(id: &str, record: &ModelRecord) -> Result<(), Error> {
        if id != record.agent_id || record.account == 0 {
            return Err(Self::corrupt_record(
                "model record key does not match its agent",
            ));
        }
        validate_agent_id(id).map_err(Self::corrupt_record)?;
        if sdk::wire::encode(record).len() > MAX_AGENT_RECORD_BYTES {
            return Err(Self::corrupt_record("model record exceeds its store bound"));
        }
        Ok(())
    }

    async fn model_ids(&self) -> Result<Vec<String>, Error> {
        self.index_ids(MODEL_INDEX_KEY, MAX_REGISTERED_AGENTS, "model registry")
            .await
    }

    async fn owner_ids(&self, owner: &RunOrigin) -> Result<Vec<String>, Error> {
        self.index_ids(
            &Self::model_owner_key(owner),
            MAX_AGENTS_PER_OWNER,
            "model owner",
        )
        .await
    }

    async fn index_ids(&self, key: &str, limit: usize, label: &str) -> Result<Vec<String>, Error> {
        let Some(bytes) = self.receipts.get(key).await? else {
            return Ok(Vec::new());
        };
        let ids: Vec<String> = sdk::wire::decode(&bytes).map_err(Self::corrupt_record)?;
        if ids.len() > limit || ids.windows(2).any(|pair| pair[0] >= pair[1]) {
            return Err(Self::corrupt_record(format!("invalid {label} index")));
        }
        for id in &ids {
            validate_agent_id(id).map_err(Self::corrupt_record)?;
        }
        Ok(ids)
    }

    fn stage_index(&mut self, key: &str, ids: &[String], limit: usize) -> Result<(), Error> {
        if ids.len() > limit || ids.windows(2).any(|pair| pair[0] >= pair[1]) {
            return Err(Error::Module {
                reason: refusal::CAPACITY.into(),
                sentence: "model index exceeds its capacity".into(),
            });
        }
        self.receipts.stage(key.into(), sdk::wire::encode(&ids))
    }

    /// Install native test fixtures through the same point-record layout used
    /// in production. Re-seeding only replaces model records, preserving any
    /// unrelated receipt writes staged by the test's current block.
    #[cfg(test)]
    pub(crate) fn seed_test_models(
        &mut self,
        models: &BTreeMap<String, ModelRecord>,
    ) -> Result<(), Error> {
        if self.legacy_models.is_some() {
            return Ok(());
        }
        let mut old_keys: BTreeSet<String> = self
            .receipts
            .snapshot()
            .into_keys()
            .filter(|key| key.starts_with("model/"))
            .collect();
        old_keys.extend(
            self.receipts
                .staged()
                .keys()
                .filter(|key| key.starts_with("model/"))
                .cloned(),
        );
        for key in old_keys {
            self.receipts.remove(key);
        }

        let ids: Vec<String> = models.keys().cloned().collect();
        let mut owners = BTreeMap::<String, Vec<String>>::new();
        for (id, record) in models {
            Self::validate_model_record(id, record)?;
            owners
                .entry(Self::model_owner_key(&record.owner))
                .or_default()
                .push(id.clone());
            self.receipts
                .stage(Self::model_item_key(id), sdk::wire::encode(record))?;
        }
        self.stage_index(MODEL_INDEX_KEY, &ids, MAX_REGISTERED_AGENTS)?;
        for (key, ids) in owners {
            self.stage_index(&key, &ids, MAX_AGENTS_PER_OWNER)?;
        }
        Ok(())
    }

    /// Move the old collection into point records at the start of the first
    /// mutable operation. The legacy map remains until commit so abort can
    /// retry the exact old state without exposing a partial migration.
    pub(super) fn stage_legacy_models(&mut self) -> Result<(), Error> {
        let Some(legacy_models) = self.legacy_models.as_ref() else {
            return Ok(());
        };
        if self.legacy_migration_staged {
            return Ok(());
        }
        let models: Vec<(String, ModelRecord)> = legacy_models
            .iter()
            .map(|(id, record)| (id.clone(), record.clone()))
            .collect();
        let ids: Vec<String> = models.iter().map(|(id, _)| id.clone()).collect();
        let mut owners = BTreeMap::<String, Vec<String>>::new();
        for (id, record) in &models {
            Self::validate_model_record(id, record)?;
            owners
                .entry(Self::model_owner_key(&record.owner))
                .or_default()
                .push(id.clone());
            self.receipts
                .stage(Self::model_item_key(id), sdk::wire::encode(record))?;
        }
        self.stage_index(MODEL_INDEX_KEY, &ids, MAX_REGISTERED_AGENTS)?;
        for (key, ids) in owners {
            self.stage_index(&key, &ids, MAX_AGENTS_PER_OWNER)?;
        }
        self.legacy_migration_staged = true;
        Ok(())
    }

    pub(super) async fn model(&self, id: &str) -> Result<Option<ModelRecord>, Error> {
        if self.legacy_models.is_some() && !self.legacy_migration_staged {
            return Ok(self
                .legacy_models
                .as_ref()
                .and_then(|models| models.get(id).cloned()));
        }
        let Some(bytes) = self.receipts.get(&Self::model_item_key(id)).await? else {
            return Ok(None);
        };
        let record: ModelRecord = sdk::wire::decode(&bytes).map_err(Self::corrupt_record)?;
        Self::validate_model_record(id, &record)?;
        Ok(Some(record))
    }

    pub(super) async fn model_records(&self) -> Result<Vec<ModelRecord>, Error> {
        if let Some(legacy_models) = self.legacy_models.as_ref()
            && !self.legacy_migration_staged
        {
            return Ok(legacy_models.values().cloned().collect());
        }
        let ids = self.model_ids().await?;
        let mut records = Vec::with_capacity(ids.len());
        for id in ids {
            let Some(record) = self.model(&id).await? else {
                return Err(Self::corrupt_record("model index names a missing record"));
            };
            records.push(record);
        }
        Ok(records)
    }

    pub(super) async fn account_control(
        &self,
        ctx: &dyn Ctx,
        account: sdk::AccountNumber,
    ) -> Result<identity::Control, Error> {
        let bytes = ctx
            .query(
                "identity",
                &identity::encode_query(&identity::IdentityQuery::Get { number: account }),
            )
            .await?;
        let identity::IdentityReply::Account(Some(view)) =
            identity::decode_reply(&bytes).map_err(|sentence| Error::Module {
                reason: refusal::UNEXPECTED_REPLY.into(),
                sentence,
            })?
        else {
            return Err(Error::Module {
                reason: refusal::NOT_FOUND.into(),
                sentence: format!("account {account} does not exist"),
            });
        };
        Ok(view.control)
    }

    pub(super) async fn active_generation(
        &self,
        ctx: &dyn Ctx,
        account: u64,
    ) -> Result<u64, Error> {
        let identity::Control::Program {
            executor,
            generation,
            standing: identity::ProgramStanding::Active,
            ..
        } = self.account_control(ctx, account).await?
        else {
            return Err(Error::Module {
                reason: refusal::WRONG_STATE.into(),
                sentence: format!("account {account} is not an active program"),
            });
        };
        if executor != self.agent {
            return Err(Error::Module {
                reason: refusal::INVALID_INPUT.into(),
                sentence: format!(
                    "program {account} is executed by {executor}, not by {}",
                    self.agent
                ),
            });
        }
        Ok(generation)
    }

    /// a model serves a live program account executed by this module's agent
    /// module; any other account has no program a run could act through.
    pub(super) async fn program_model(
        &self,
        ctx: &dyn Ctx,
        account: sdk::AccountNumber,
    ) -> Result<(), Error> {
        let identity::Control::Program { executor, .. } =
            self.account_control(ctx, account).await?
        else {
            return Err(Error::Module {
                reason: refusal::INVALID_INPUT.into(),
                sentence: format!("account {account} is not a program account"),
            });
        };
        if executor != self.agent {
            return Err(Error::Module {
                reason: refusal::INVALID_INPUT.into(),
                sentence: format!(
                    "program {account} is executed by {executor}, not by {}",
                    self.agent
                ),
            });
        }
        Ok(())
    }

    fn stage_model(&mut self, record: ModelRecord) -> Result<(), Error> {
        let bytes = sdk::wire::encode(&record);
        if bytes.len() > MAX_AGENT_RECORD_BYTES {
            return Err(Error::Module {
                reason: refusal::CAPACITY.into(),
                sentence: format!("model record exceeds {MAX_AGENT_RECORD_BYTES} bytes"),
            });
        }
        self.receipts
            .stage(Self::model_item_key(&record.agent_id), bytes)
    }

    async fn registered_model(&self, id: &str) -> Result<ModelRecord, Error> {
        self.model(id).await?.ok_or_else(|| Error::Module {
            reason: refusal::NOT_FOUND.into(),
            sentence: format!("unknown model: {id}"),
        })
    }

    pub(super) async fn configure_model(
        &mut self,
        ctx: &mut dyn Ctx,
        operation: ModelMsg,
    ) -> Result<(), Error> {
        match operation {
            ModelMsg::RegisterModel {
                account,
                agent_id,
                display_name,
                capability,
                recipe_hash,
                skills,
            } => {
                self.program_model(ctx, account).await?;
                validate_agent_id(&agent_id).map_err(|sentence| Error::Module {
                    reason: refusal::INVALID_INPUT.into(),
                    sentence,
                })?;
                Self::validate_non_empty("display_name", &display_name)?;
                validate_tag(&capability).map_err(|sentence| Error::Module {
                    reason: refusal::INVALID_INPUT.into(),
                    sentence,
                })?;
                if self.model(&agent_id).await?.is_some() {
                    return Err(Error::Module {
                        reason: refusal::ALREADY_EXISTS.into(),
                        sentence: format!("model already exists: {agent_id}"),
                    });
                }
                let mut ids = self.model_ids().await?;
                if ids.len() >= MAX_REGISTERED_AGENTS {
                    return Err(Error::Module {
                        reason: refusal::CAPACITY.into(),
                        sentence: format!(
                            "the model registry already holds {MAX_REGISTERED_AGENTS} models, its limit"
                        ),
                    });
                }
                let owner = canonical_origin(&ctx.env().origin)?;
                let mut owned_ids = self.owner_ids(&owner).await?;
                if owned_ids.len() >= MAX_AGENTS_PER_OWNER {
                    return Err(Error::Module {
                        reason: refusal::CAPACITY.into(),
                        sentence: format!(
                            "this owner already has {MAX_AGENTS_PER_OWNER} models, the most one owner may register"
                        ),
                    });
                }
                let recipe_hash = recipe_hash.unwrap_or_default();
                Self::validate_recipe_hash(&recipe_hash)?;
                let skills = skills.unwrap_or_default();
                Self::validate_skills(&skills)?;
                let record = ModelRecord {
                    account,
                    agent_id: agent_id.clone(),
                    owner: owner.clone(),
                    display_name,
                    capability: capability.clone(),
                    status: ModelStatus::Active,
                    role: ModelRole::default(),
                    created_at: ctx.env().consensus_time,
                    updated_at: ctx.env().consensus_time,
                    recipe_hash,
                    skills,
                };
                self.stage_model(record)?;
                ids.push(agent_id.clone());
                ids.sort_unstable();
                self.stage_index(MODEL_INDEX_KEY, &ids, MAX_REGISTERED_AGENTS)?;
                owned_ids.push(agent_id.clone());
                owned_ids.sort_unstable();
                self.stage_index(
                    &Self::model_owner_key(&owner),
                    &owned_ids,
                    MAX_AGENTS_PER_OWNER,
                )?;
                self.apply_model_change(
                    ctx,
                    ModelChange::Registered {
                        agent_id,
                        capability,
                    },
                )
            }
            ModelMsg::UpdateModel {
                agent_id,
                display_name,
                capability,
                recipe_hash,
                skills,
            } => {
                let mut record = self.registered_model(&agent_id).await?;
                if let Some(name) = display_name {
                    Self::validate_non_empty("display_name", &name)?;
                    record.display_name = name;
                }
                if let Some(capability) = capability {
                    validate_tag(&capability).map_err(|sentence| Error::Module {
                        reason: refusal::INVALID_INPUT.into(),
                        sentence,
                    })?;
                    if capability != record.capability {
                        self.apply_model_change(
                            ctx,
                            ModelChange::CapabilityChanged {
                                agent_id: agent_id.clone(),
                                capability: capability.clone(),
                            },
                        )?;
                    }
                    record.capability = capability;
                }
                if let Some(hash) = recipe_hash {
                    Self::validate_recipe_hash(&hash)?;
                    record.recipe_hash = hash;
                }
                if let Some(skills) = skills {
                    Self::validate_skills(&skills)?;
                    record.skills = skills;
                }
                record.updated_at = ctx.env().consensus_time;
                self.stage_model(record)
            }
            ModelMsg::PauseModel { agent_id } => {
                self.set_model_status(ctx, agent_id, ModelStatus::Paused)
                    .await
            }
            ModelMsg::ResumeModel { agent_id } => {
                self.set_model_status(ctx, agent_id, ModelStatus::Active)
                    .await
            }
            ModelMsg::DeregisterModel { agent_id } => {
                let record = self.registered_model(&agent_id).await?;
                let mut ids = self.model_ids().await?;
                ids.retain(|id| id != &agent_id);
                self.stage_index(MODEL_INDEX_KEY, &ids, MAX_REGISTERED_AGENTS)?;
                let mut owned_ids = self.owner_ids(&record.owner).await?;
                owned_ids.retain(|id| id != &agent_id);
                if owned_ids.is_empty() {
                    self.receipts.remove(Self::model_owner_key(&record.owner));
                } else {
                    self.stage_index(
                        &Self::model_owner_key(&record.owner),
                        &owned_ids,
                        MAX_AGENTS_PER_OWNER,
                    )?;
                }
                self.receipts.remove(Self::model_item_key(&agent_id));
                self.apply_model_change(ctx, ModelChange::Deregistered { agent_id })
            }
        }
    }

    async fn set_model_status(
        &mut self,
        ctx: &dyn Ctx,
        id: String,
        status: ModelStatus,
    ) -> Result<(), Error> {
        let mut record = self.registered_model(&id).await?;
        record.status = status;
        record.updated_at = ctx.env().consensus_time;
        self.stage_model(record)
    }
}
