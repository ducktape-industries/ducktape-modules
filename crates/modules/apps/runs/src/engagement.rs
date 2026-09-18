//! Source context is read only after the user's program explicitly requests model work.
use super::*;

/// Only runs publishes this detail. Manual request ownership is stamped from
/// its authenticated origin before the program chooses whether to execute.
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum RunRequest {
    Conversation {
        agent_id: String,
        conversation_id: String,
        turn: u64,
    },
    Manual {
        requester: RunOrigin,
        agent_id: String,
        channel_id: String,
        anchor_seq: u64,
        demands: BTreeMap<String, u64>,
        skills: Vec<String>,
    },
    Job {
        agent_id: String,
        job_id: String,
    },
}

impl RunsModule {
    pub(super) async fn request_attributed_run(
        &mut self,
        ctx: &mut dyn Ctx,
        agent_id: String,
        change_seq: u64,
        budget: &SiblingReadBudget,
    ) -> Result<(), Error> {
        let Origin::Program(account) = ctx.env().origin else {
            return Err(Error::Module {
                reason: "model_work_origin".into(),
                sentence: "attributed model work requires a program call".into(),
            });
        };
        let Some(model) = self
            .active_agent(&*ctx, &agent_id)
            .await
            .map_err(|sentence| Error::Module {
                reason: "active_agent".into(),
                sentence,
            })?
        else {
            return Err(Error::Module {
                reason: "model_is_not_active".into(),
                sentence: format!("model {agent_id} is not active"),
            });
        };
        if model.account != account {
            return Err(Error::Module {
                reason: "model_belongs_to_another_account".into(),
                sentence: format!(
                    "model {agent_id} belongs to account {}, not {account}",
                    model.account
                ),
            });
        }
        let after = change_seq.checked_sub(1).ok_or_else(|| Error::Module {
            reason: "attribution_changes_start_at_one".into(),
            sentence: "attribution changes are numbered from 1, so change 0 does not exist".into(),
        })?;
        let bytes = ctx
            .query(
                &self.attribution,
                &attribution::encode_query(&attribution::AttributionQuery::Changes {
                    after,
                    limit: 1,
                }),
            )
            .await?;
        let attribution::AttributionReply::Changes(changes) = attribution::decode_reply(&bytes)
            .map_err(|sentence| Error::Module {
                reason: "codec".into(),
                sentence,
            })?
        else {
            return Err(Error::Module {
                reason: "unexpected_attribution_reply".into(),
                sentence:
                    "attribution answered the change lookup with something other than changes"
                        .into(),
            });
        };
        let Some(entry) = changes.first() else {
            return Err(Error::Module {
                reason: "attribution_does_not_exist".into(),
                sentence: format!("no attribution change {change_seq}"),
            });
        };
        let change = &entry.change;
        let addressed = change.seq == change_seq && change.recipient == account;
        if !addressed {
            return Err(Error::Module {
                reason: "attribution_belongs_to_another_account".into(),
                sentence: format!(
                    "attribution change {change_seq} is not addressed to account {account}"
                ),
            });
        }
        let own_request = change.source.module == self.id && change.source.kind == "run_request";
        if !own_request && let Some(conversation) = self.conversation_for_account(account).await? {
            return self
                .admit_conversation_attribution(ctx, &conversation, change)
                .await;
        }
        let run_id = if own_request {
            match sdk::wire::decode::<RunRequest>(&change.detail).map_err(|sentence| {
                Error::Module {
                    reason: "codec".into(),
                    sentence,
                }
            })? {
                RunRequest::Conversation {
                    agent_id: requested,
                    conversation_id,
                    turn,
                } => {
                    if requested != agent_id {
                        return Err(Error::Module {
                            reason: "conversation_model_mismatch".into(),
                            sentence: format!(
                                "conversation request names model {requested}, not {agent_id}"
                            ),
                        });
                    }
                    return self
                        .request_conversation_turn(ctx, conversation_id, turn)
                        .await;
                }
                RunRequest::Job {
                    agent_id: requested,
                    job_id,
                } => {
                    if requested != agent_id {
                        return Err(Error::Module {
                            reason: "job_request_names_another_model".into(),
                            sentence: format!(
                                "job request names model {requested}, not {agent_id}"
                            ),
                        });
                    }
                    return self.request_job_run(ctx, agent_id, job_id).await;
                }
                RunRequest::Manual {
                    agent_id: requested,
                    channel_id,
                    anchor_seq,
                    ..
                } => {
                    if requested != agent_id {
                        return Err(Error::Module {
                            reason: "run_request_names_another_model".into(),
                            sentence: format!(
                                "run request names model {requested}, not {agent_id}"
                            ),
                        });
                    }
                    run_id_for(&channel_id, anchor_seq, &agent_id)
                }
            }
        } else {
            format!("attributed/{change_seq}/{agent_id}")
        };
        if self
            .turn_taken(&*ctx, &dispatch_id_for(&run_id))
            .await
            .map_err(|sentence| Error::Module {
                reason: "dispatch_turn".into(),
                sentence,
            })?
        {
            return Ok(());
        }
        let (channel, anchor, prepared, demands, requester) =
            match (change.source.module.as_str(), change.source.kind.as_str()) {
                (source, "message") if source == self.chat => {
                    let bytes = ctx
                        .query(
                            &self.chat,
                            &chat_encode_query(&ChatQuery::Message {
                                message_id: change.source.object.clone(),
                            }),
                        )
                        .await?;
                    let ChatReply::Message(Some(message)) =
                        chat_decode_reply(&bytes).map_err(|sentence| Error::Module {
                            reason: "codec".into(),
                            sentence,
                        })?
                    else {
                        return Err(Error::Module {
                            reason: "attributed_chat_message_is_unavailable".into(),
                            sentence: format!(
                                "attributed chat message {} is unavailable",
                                change.source.object
                            ),
                        });
                    };
                    let channel = message.channel_id.clone();
                    let prepared = self
                        .prepare_dispatch(
                            &*ctx,
                            &model,
                            &run_id,
                            &channel,
                            message.seq,
                            &[],
                            budget,
                        )
                        .await
                        .map_err(|sentence| Error::Module {
                            reason: "execution_budget".into(),
                            sentence,
                        })?;
                    (
                        channel,
                        message.seq,
                        prepared,
                        BTreeMap::new(),
                        RunOrigin::Program(account),
                    )
                }
                (source, "block") if Some(source) == self.pages.as_deref() => {
                    let prepared = self
                        .prepare_page_block_dispatch(
                            &*ctx,
                            &model,
                            &run_id,
                            &change.source.object,
                            budget,
                        )
                        .await
                        .map_err(|sentence| Error::Module {
                            reason: "execution_budget".into(),
                            sentence,
                        })?;
                    (
                        page_block_channel_id(&change.source.object),
                        change.revision,
                        prepared,
                        BTreeMap::new(),
                        RunOrigin::Program(account),
                    )
                }
                (source, "comment") if Some(source) == self.pages.as_deref() => {
                    let bytes = ctx
                        .query(
                            source,
                            &pages::encode_query(&pages::PageQuery::GetComment {
                                comment_id: change.source.object.clone(),
                            }),
                        )
                        .await?;
                    let pages::PageReply::Comment(Some(comment)) = pages::decode_reply(&bytes)
                        .map_err(|sentence| Error::Module {
                            reason: "codec".into(),
                            sentence,
                        })?
                    else {
                        return Err(Error::Module {
                            reason: "attributed_page_comment_is_unavailable".into(),
                            sentence: format!(
                                "attributed page comment {} is unavailable",
                                change.source.object
                            ),
                        });
                    };
                    let bytes = ctx
                        .query(
                            source,
                            &pages::encode_query(&pages::PageQuery::CommentThread {
                                thread_id: comment.thread_id.clone(),
                            }),
                        )
                        .await?;
                    let pages::PageReply::CommentThread(Some(thread)) = pages::decode_reply(&bytes)
                        .map_err(|sentence| Error::Module {
                            reason: "codec".into(),
                            sentence,
                        })?
                    else {
                        return Err(Error::Module {
                            reason: "attributed_comment_thread_is_unavailable".into(),
                            sentence: format!(
                                "comment thread {} is unavailable",
                                comment.thread_id
                            ),
                        });
                    };
                    let Some(index) = thread
                        .comments
                        .iter()
                        .position(|item| item.id == comment.id)
                    else {
                        return Err(Error::Module {
                            reason: "comment_thread_mismatch".into(),
                            sentence: "comment is not in its thread".into(),
                        });
                    };
                    let ordinal = index as u64 + 1;
                    let prepared = self
                        .prepare_page_dispatch(
                            &*ctx,
                            &model,
                            &run_id,
                            &comment.thread_id,
                            ordinal,
                            budget,
                        )
                        .await
                        .map_err(|sentence| Error::Module {
                            reason: "execution_budget".into(),
                            sentence,
                        })?;
                    (
                        page_channel_id(&comment.thread_id),
                        ordinal,
                        prepared,
                        BTreeMap::new(),
                        RunOrigin::Program(account),
                    )
                }
                (source, "run_request") if source == self.id => {
                    let RunRequest::Manual {
                        requester,
                        agent_id: requested,
                        channel_id,
                        anchor_seq,
                        demands,
                        skills,
                    } = sdk::wire::decode(&change.detail).map_err(|sentence| Error::Module {
                        reason: "codec".into(),
                        sentence,
                    })?
                    else {
                        return Err(Error::Module {
                            reason: "unexpected_run_request_detail".into(),
                            sentence: "this run request's detail is not a manual run request"
                                .into(),
                        });
                    };
                    if requested != agent_id {
                        return Err(Error::Module {
                            reason: "run_request_names_another_model".into(),
                            sentence: format!(
                                "run request names model {requested}, not {agent_id}"
                            ),
                        });
                    }
                    let skills =
                        envelope::library_skills(&skills).map_err(|sentence| Error::Module {
                            reason: "library_skills".into(),
                            sentence,
                        })?;
                    let prepared = self
                        .prepare_dispatch(
                            &*ctx,
                            &model,
                            &run_id,
                            &channel_id,
                            anchor_seq,
                            &skills,
                            budget,
                        )
                        .await
                        .map_err(|sentence| Error::Module {
                            reason: "execution_budget".into(),
                            sentence,
                        })?;
                    (channel_id, anchor_seq, prepared, demands, requester)
                }
                _ => {
                    return Err(Error::Module {
                        reason: "model_workflow_composer".into(),
                        sentence: "this model workflow has no composer for the attribution source"
                            .into(),
                    });
                }
            };
        self.stage_dispatch_run(
            ctx, &run_id, agent_id, channel, anchor, requester, prepared, demands,
        );
        ctx.set_output(sdk::wire::encode(&serde_json::json!({"run_id": run_id})));
        Ok(())
    }
}
