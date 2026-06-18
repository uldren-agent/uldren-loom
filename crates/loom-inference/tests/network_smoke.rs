#![cfg(all(feature = "candle-cpu", feature = "native-hf"))]

use std::path::PathBuf;

use loom_inference::{
    DownloadEvent, DownloadJobManager, DownloadJobPlan, HfCachePaths, HfDownloader,
    InstalledModelRecord, LlmRequest, Message, activate_llm, activate_text_embedding,
    curated_models, discover_installed_model, probe_hardware,
};
use loom_types::{Result, RuntimeKind};

#[test]
#[ignore = "downloads Hugging Face model files and runs Candle on local hardware"]
fn downloaded_embedding_model_activates_with_candle() {
    let (cache_dir, record) =
        download_curated_model("sentence-transformers/all-MiniLM-L6-v2", "embedding").unwrap();
    let mut hardware = probe_hardware().unwrap();
    hardware.hf_cache_dir = Some(cache_dir.to_string_lossy().into_owned());

    let embeddings = activate_text_embedding(&record, &hardware, &cache_dir).unwrap();
    let model = embeddings.model().unwrap();
    let vectors = embeddings
        .embed(&["alpha".to_string(), "beta".to_string()])
        .unwrap();

    assert_eq!(model.dimension, 384);
    assert_eq!(vectors.len(), 2);
    assert_eq!(vectors[0].len(), 384);
    assert_ne!(vectors[0], vectors[1]);
    assert!(vectors.iter().flatten().all(|value| value.is_finite()));
}

#[test]
#[ignore = "downloads Hugging Face model files and runs Candle on local hardware"]
fn downloaded_qwen2_model_completes_with_candle() {
    let (cache_dir, record) =
        download_curated_model("Qwen/Qwen2.5-0.5B-Instruct", "qwen2").unwrap();
    let mut hardware = probe_hardware().unwrap();
    hardware.hf_cache_dir = Some(cache_dir.to_string_lossy().into_owned());

    let llm = activate_llm(&record, &hardware, &cache_dir).unwrap();
    let response = llm
        .complete(&LlmRequest {
            messages: vec![Message::user("Say ok.")],
            max_tokens: Some(1),
            ..LlmRequest::default()
        })
        .unwrap();

    assert_eq!(response.model, llm.model_id().unwrap());
    assert!(response.content.is_char_boundary(response.content.len()));
}

fn download_curated_model(repo_id: &str, tag: &str) -> Result<(PathBuf, InstalledModelRecord)> {
    let spec = curated_models()
        .iter()
        .copied()
        .find(|spec| spec.repo_id == repo_id && spec.runtime == RuntimeKind::CandleSafetensors)
        .unwrap();
    let cache_dir = HfCachePaths::from_env()?.cache_dir;
    let manager = DownloadJobManager::new(cache_dir.clone()).with_max_retries(1);
    let plan = DownloadJobPlan::new(
        spec.model_ref(),
        spec.runtime,
        spec.files.iter().map(|file| file.to_string()).collect(),
    )?;
    let job_id = format!("network-smoke-{tag}-{}", std::process::id());
    manager.enqueue_with_id(job_id.clone(), plan)?;
    let downloader = HfDownloader::from_env(std::env::var("HF_TOKEN").ok())?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;

    runtime.block_on(manager.run_hf(&job_id, &downloader, print_smoke_event))?;
    let model = loom_types::ModelRef::new(spec.kind, repo_id)
        .with_revision(loom_types::RevisionRef::Branch(spec.revision.to_string()));
    let record = discover_installed_model(&cache_dir, &model, spec.runtime)?.unwrap();
    Ok((cache_dir, record))
}

fn print_smoke_event(event: DownloadEvent) {
    match event {
        DownloadEvent::StateChanged { job_id, state } => {
            eprintln!("job\t{job_id}\tstate={}", state.as_str());
        }
        DownloadEvent::FileStarted { job_id, file } => {
            eprintln!("job\t{job_id}\tfile={file}\tstate=started");
        }
        DownloadEvent::FileFinished {
            job_id,
            file,
            size_bytes,
            ..
        } => {
            eprintln!("job\t{job_id}\tfile={file}\tbytes={size_bytes}");
        }
        DownloadEvent::Retry {
            job_id,
            file,
            attempt,
            message,
        } => {
            eprintln!("job\t{job_id}\tfile={file}\tretry={attempt}\tmessage={message}");
        }
        DownloadEvent::Failed { job_id, message } => {
            eprintln!("job\t{job_id}\tstate=failed\tmessage={message}");
        }
    }
}
