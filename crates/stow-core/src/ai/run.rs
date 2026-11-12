//! Execute an `ai_runs` row: gather context, prompt the provider, store the output.

use crate::ai::{build_prompt, provider_from_settings, AiClient, AiContext};
use crate::jobs::{JobEvent, JobHook};
use crate::models::ParentType;
use crate::repo::{ai_runs, buckets, items, recordings};
use crate::{Db, Result};

/// Everything the prompt builder may want about an item or bucket.
pub fn load_context(db: &Db, parent_type: ParentType, parent_id: &str) -> Result<AiContext> {
    let mut ctx = AiContext::default();
    match parent_type {
        ParentType::Item => {
            let it = items::get(db, parent_id)?;
            ctx.title.clone_from(&it.title);
            ctx.url = Some(it.url.clone());
            ctx.bucket_name = it
                .bucket_id
                .as_deref()
                .and_then(|b| buckets::get(db, b).ok())
                .map(|b| b.name);
            ctx.notes = Some(it.notes_md.clone()).filter(|n| !n.trim().is_empty());
            ctx.snapshot_text = crate::fetch::snapshot::read_snapshot_md(db.data_dir(), parent_id)?;
        }
        ParentType::Bucket => {
            let b = buckets::get(db, parent_id)?;
            ctx.title = Some(b.name.clone());
            ctx.bucket_name = Some(b.name.clone());
            let notes = format!("{}\n{}", b.description_md, b.notes_md);
            ctx.notes = Some(notes.trim().to_string()).filter(|n| !n.is_empty());
        }
    }
    // Latest finished transcript, if any.
    ctx.transcript = recordings::list(db, parent_type, parent_id)?
        .into_iter()
        .find_map(|r| r.transcript_md)
        .filter(|t| !t.trim().is_empty());
    Ok(ctx)
}

/// Mark the run failed and tell the UI.
pub(crate) fn fail_run(db: &Db, run_id: &str, err: &str, hook: &JobHook) -> Result<()> {
    ai_runs::fail(db, run_id, err)?;
    hook(JobEvent::AiRunUpdated(run_id.to_string()));
    Ok(())
}

/// Run with an explicit provider (testable without the Keychain or network).
pub async fn run_ai_with(
    db: &Db,
    run_id: &str,
    provider: &dyn AiClient,
    hook: &JobHook,
) -> Result<()> {
    let run = ai_runs::get(db, run_id)?;
    ai_runs::start(db, run_id)?;
    hook(JobEvent::AiRunUpdated(run_id.to_string()));
    let outcome: Result<String> = async {
        let ctx = load_context(db, run.parent_type, &run.parent_id)?;
        let (system, user) = build_prompt(run.action, &ctx);
        provider.complete(&system, &user).await
    }
    .await;
    match outcome {
        Ok(text) => {
            ai_runs::complete(db, run_id, text.trim())?;
            hook(JobEvent::AiRunUpdated(run_id.to_string()));
            Ok(())
        }
        Err(e) => {
            fail_run(db, run_id, &e.to_string(), hook)?;
            Err(e)
        }
    }
}

/// Job entry point: provider comes from settings + Keychain.
#[tracing::instrument(skip_all, fields(run_id = %run_id))]
pub async fn run_ai(db: &Db, run_id: &str, hook: &JobHook) -> Result<()> {
    let provider = match provider_from_settings(db) {
        Ok(p) => p,
        Err(e) => {
            fail_run(db, run_id, &e.to_string(), hook)?;
            return Err(e);
        }
    };
    run_ai_with(db, run_id, provider.as_ref(), hook).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jobs::{JobEvent, JobQueue};
    use crate::models::{AiAction, JobStatus, ParentType, Provider};
    use crate::repo::{ai_runs, buckets, items, recordings, test_db};
    use std::sync::{Arc, Mutex};

    struct MockProvider {
        reply: String,
        seen: Arc<Mutex<Vec<(String, String)>>>,
    }
    #[async_trait::async_trait]
    impl AiClient for MockProvider {
        async fn complete(&self, system: &str, user: &str) -> Result<String> {
            self.seen
                .lock()
                .unwrap()
                .push((system.to_string(), user.to_string()));
            if self.reply == "FAIL" {
                return Err(crate::StowError::Provider {
                    provider: "anthropic",
                    status: Some(500),
                    message: "boom".into(),
                });
            }
            Ok(self.reply.clone())
        }
        fn provider(&self) -> crate::models::Provider {
            crate::models::Provider::Anthropic
        }
        fn model(&self) -> &'static str {
            "mock-1"
        }
    }

    #[test]
    fn load_context_for_item_includes_notes_snapshot_and_transcript() {
        let (_d, db) = test_db();
        let b = buckets::create(&db, "Blog", None).unwrap();
        let it = items::create(
            &db,
            items::NewItem {
                url: "https://example.com/p".into(),
                bucket_id: Some(b.id.clone()),
                title: Some("Post".into()),
            },
        )
        .unwrap()
        .value;
        items::set_notes(&db, &it.id, "my notes here").unwrap();
        std::fs::write(
            db.data_dir()
                .join("snapshots")
                .join(format!("{}.md", it.id)),
            "# Post\n\nsnapshot body",
        )
        .unwrap();
        let rec = recordings::create(&db, ParentType::Item, &it.id, "/tmp/x.wav", 1000).unwrap();
        recordings::set_transcript(&db, &rec.id, "**[00:00]** spoken words").unwrap();
        let ctx = load_context(&db, ParentType::Item, &it.id).unwrap();
        assert_eq!(ctx.title.as_deref(), Some("Post"));
        assert_eq!(ctx.url.as_deref(), Some("https://example.com/p"));
        assert_eq!(ctx.bucket_name.as_deref(), Some("Blog"));
        assert_eq!(ctx.notes.as_deref(), Some("my notes here"));
        assert!(ctx.snapshot_text.unwrap().contains("snapshot body"));
        assert!(ctx.transcript.unwrap().contains("spoken words"));
    }

    #[test]
    fn load_context_for_bucket_uses_bucket_notes() {
        let (_d, db) = test_db();
        let b = buckets::create(&db, "Blog", None).unwrap();
        buckets::update(
            &db,
            &b.id,
            &buckets::BucketPatch {
                notes_md: Some("bucket notes".into()),
                ..Default::default()
            },
        )
        .unwrap();
        let ctx = load_context(&db, ParentType::Bucket, &b.id).unwrap();
        assert_eq!(ctx.title.as_deref(), Some("Blog"));
        assert_eq!(ctx.notes.as_deref(), Some("bucket notes"));
        assert!(ctx.snapshot_text.is_none());
    }

    #[tokio::test]
    async fn run_ai_with_completes_run_and_emits_event() {
        let (_d, db) = test_db();
        let it = items::create(
            &db,
            items::NewItem {
                url: "https://example.com/p".into(),
                bucket_id: None,
                title: Some("Post".into()),
            },
        )
        .unwrap()
        .value;
        items::set_notes(&db, &it.id, "zebra notes").unwrap();
        let run = ai_runs::create(
            &db,
            ParentType::Item,
            &it.id,
            AiAction::SummarizeNotes,
            Provider::Anthropic,
            "mock-1",
            "h",
        )
        .unwrap();
        let seen = Arc::new(Mutex::new(vec![]));
        let p = MockProvider {
            reply: "- summary".into(),
            seen: seen.clone(),
        };
        let (tx, rx) = std::sync::mpsc::channel();
        let hook: crate::jobs::JobHook = Arc::new(move |e| {
            let _ = tx.send(e);
        });
        run_ai_with(&db, &run.id, &p, &hook).await.unwrap();
        let got = ai_runs::get(&db, &run.id).unwrap();
        assert_eq!(got.status, JobStatus::Done);
        assert_eq!(got.output_md.as_deref(), Some("- summary"));
        assert!(seen.lock().unwrap()[0].1.contains("zebra notes"));
        assert!(matches!(rx.try_recv(), Ok(JobEvent::AiRunUpdated(id)) if id == run.id));
    }

    #[tokio::test]
    async fn run_ai_with_failure_marks_failed() {
        let (_d, db) = test_db();
        let it = items::create(
            &db,
            items::NewItem {
                url: "https://example.com/q".into(),
                bucket_id: None,
                title: None,
            },
        )
        .unwrap()
        .value;
        let run = ai_runs::create(
            &db,
            ParentType::Item,
            &it.id,
            AiAction::SuggestTasks,
            Provider::Anthropic,
            "mock-1",
            "h",
        )
        .unwrap();
        let p = MockProvider {
            reply: "FAIL".into(),
            seen: Arc::default(),
        };
        let r = run_ai_with(&db, &run.id, &p, &JobQueue::noop_hook()).await;
        assert!(r.is_err());
        let got = ai_runs::get(&db, &run.id).unwrap();
        assert_eq!(got.status, JobStatus::Failed);
        assert!(got.error.unwrap().contains("boom"));
    }

    #[tokio::test]
    async fn run_ai_without_key_fails_run_with_message() {
        let (_d, db) = test_db();
        let it = items::create(
            &db,
            items::NewItem {
                url: "https://example.com/r".into(),
                bucket_id: None,
                title: None,
            },
        )
        .unwrap()
        .value;
        let run = ai_runs::create(
            &db,
            ParentType::Item,
            &it.id,
            AiAction::SummarizeNotes,
            Provider::Anthropic,
            "m",
            "h",
        )
        .unwrap();
        // select_provider with no key is what provider_from_settings would hit; exercise that path directly.
        let provider = crate::ai::select_provider(crate::ai::ProviderConfig {
            provider: Provider::Anthropic,
            model: None,
            api_key: None,
            base_url: None,
        });
        assert!(provider.is_err());
        fail_run(
            &db,
            &run.id,
            &provider.err().unwrap().to_string(),
            &JobQueue::noop_hook(),
        )
        .unwrap();
        assert_eq!(
            ai_runs::get(&db, &run.id).unwrap().status,
            JobStatus::Failed
        );
    }
}
