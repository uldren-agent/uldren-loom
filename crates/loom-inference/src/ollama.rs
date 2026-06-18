//! Optional Ollama adapter backed by `ollama-rs`.

use loom_types::{Code, LoomError, Result};
use ollama_rs::Ollama;
use ollama_rs::generation::completion::request::GenerationRequest;
use ollama_rs::generation::embeddings::request::{EmbeddingsInput, GenerateEmbeddingsRequest};
use ollama_rs::models::ModelOptions;
use tokio::runtime::{Builder, Runtime};

use crate::{Llm, LlmRequest, LlmResponse, OllamaAdapter, OllamaModelInfo, TextEmbedding};

pub struct OllamaRsAdapter {
    client: Ollama,
    runtime: Runtime,
}

impl OllamaRsAdapter {
    pub fn localhost() -> Result<Self> {
        Self::from_client(Ollama::builder().build())
    }

    pub fn from_client(client: Ollama) -> Result<Self> {
        let runtime = build_runtime()?;
        Ok(Self { client, runtime })
    }
}

pub struct OllamaRsLlm {
    model: String,
    client: Ollama,
    runtime: Runtime,
}

impl OllamaRsLlm {
    pub fn localhost(model: impl Into<String>) -> Result<Self> {
        Self::from_client(model, Ollama::builder().build())
    }

    pub fn from_client(model: impl Into<String>, client: Ollama) -> Result<Self> {
        Ok(Self {
            model: model.into(),
            client,
            runtime: build_runtime()?,
        })
    }
}

impl Llm for OllamaRsLlm {
    fn model_id(&self) -> &str {
        &self.model
    }

    fn complete(&self, request: &LlmRequest) -> Result<LlmResponse> {
        let mut ollama_request =
            GenerationRequest::new(self.model.clone(), prompt_from_request(request));
        if let Some(system) = &request.system_prompt {
            ollama_request = ollama_request.system(system.clone());
        }
        let options = model_options_from_request(request);
        if options != ModelOptions::default() {
            ollama_request = ollama_request.options(options);
        }
        let response = self
            .runtime
            .block_on(self.client.generate(ollama_request))
            .map_err(ollama_error)?;
        Ok(LlmResponse {
            model: response.model,
            content: response.response,
            stop_reason: response.done.then_some("done".to_string()),
        })
    }
}

pub struct OllamaRsTextEmbedding {
    model: String,
    dimension: usize,
    client: Ollama,
    runtime: Runtime,
}

impl OllamaRsTextEmbedding {
    pub fn localhost(model: impl Into<String>, dimension: usize) -> Result<Self> {
        Self::from_client(model, dimension, Ollama::builder().build())
    }

    pub fn from_client(model: impl Into<String>, dimension: usize, client: Ollama) -> Result<Self> {
        Ok(Self {
            model: model.into(),
            dimension,
            client,
            runtime: build_runtime()?,
        })
    }
}

impl TextEmbedding for OllamaRsTextEmbedding {
    fn model_id(&self) -> &str {
        &self.model
    }

    fn dimension(&self) -> usize {
        self.dimension
    }

    fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        let request = GenerateEmbeddingsRequest::new(
            self.model.clone(),
            EmbeddingsInput::Multiple(texts.to_vec()),
        );
        let response = self
            .runtime
            .block_on(self.client.generate_embeddings(request))
            .map_err(ollama_error)?;
        if response
            .embeddings
            .iter()
            .any(|embedding| embedding.len() != self.dimension)
        {
            return Err(LoomError::invalid(format!(
                "ollama embedding dimension mismatch for {}",
                self.model
            )));
        }
        Ok(response.embeddings)
    }
}

impl OllamaAdapter for OllamaRsAdapter {
    fn list_models(&self) -> Result<Vec<OllamaModelInfo>> {
        let models = self
            .runtime
            .block_on(self.client.list_local_models())
            .map_err(ollama_error)?;
        Ok(models
            .into_iter()
            .map(|model| OllamaModelInfo {
                name: model.name,
                digest: None,
                size_bytes: Some(model.size),
                modified_at: Some(model.modified_at),
            })
            .collect())
    }

    fn show_model(&self, name: &str) -> Result<Option<OllamaModelInfo>> {
        let _model = self
            .runtime
            .block_on(self.client.show_model_info(name.to_string()))
            .map_err(ollama_error)?;
        Ok(Some(OllamaModelInfo {
            name: name.to_string(),
            digest: None,
            size_bytes: None,
            modified_at: None,
        }))
    }

    fn pull_model(&self, name: &str) -> Result<()> {
        self.runtime
            .block_on(self.client.pull_model(name.to_string(), false))
            .map(|_| ())
            .map_err(ollama_error)
    }

    fn delete_model(&self, name: &str) -> Result<()> {
        self.runtime
            .block_on(self.client.delete_model(name.to_string()))
            .map_err(ollama_error)
    }
}

fn ollama_error(error: ollama_rs::error::OllamaError) -> LoomError {
    LoomError::new(Code::Io, format!("ollama api error: {error}"))
}

fn build_runtime() -> Result<Runtime> {
    Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| LoomError::new(Code::Internal, format!("create tokio runtime: {e}")))
}

fn prompt_from_request(request: &LlmRequest) -> String {
    request
        .messages
        .iter()
        .map(|message| match message.role {
            crate::Role::User => format!("User: {}", message.content),
            crate::Role::Assistant => format!("Assistant: {}", message.content),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn model_options_from_request(request: &LlmRequest) -> ModelOptions {
    let mut options = ModelOptions::default();
    if let Some(max_tokens) = request.max_tokens {
        let max_tokens = i32::try_from(max_tokens).unwrap_or(i32::MAX);
        options = options.num_predict(max_tokens);
    }
    if let Some(temperature) = request.temperature {
        options = options.temperature(temperature);
    }
    options
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::thread;

    fn fixture_client(response: &'static str) -> (Ollama, thread::JoinHandle<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let request = read_http_request(&mut stream);
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                response.len(),
                response
            );
            stream.write_all(response.as_bytes()).unwrap();
            request
        });
        let client = Ollama::builder().url(format!("http://{addr}")).build();
        (client, handle)
    }

    fn read_http_request(stream: &mut TcpStream) -> String {
        let mut request = Vec::new();
        let mut buffer = [0_u8; 1024];
        let mut header_end = None;
        loop {
            let read = stream.read(&mut buffer).unwrap();
            if read == 0 {
                break;
            }
            request.extend_from_slice(&buffer[..read]);
            header_end = request.windows(4).position(|window| window == b"\r\n\r\n");
            if header_end.is_some() {
                break;
            }
        }
        if let Some(header_end) = header_end {
            let headers = String::from_utf8_lossy(&request[..header_end]);
            let content_length = headers
                .lines()
                .find_map(|line| line.strip_prefix("content-length:"))
                .or_else(|| {
                    headers
                        .lines()
                        .find_map(|line| line.strip_prefix("Content-Length:"))
                })
                .and_then(|value| value.trim().parse::<usize>().ok())
                .unwrap_or(0);
            let target_len = header_end + 4 + content_length;
            while request.len() < target_len {
                let read = stream.read(&mut buffer).unwrap();
                if read == 0 {
                    break;
                }
                request.extend_from_slice(&buffer[..read]);
            }
        }
        String::from_utf8_lossy(&request).into_owned()
    }

    #[test]
    fn constructs_localhost_adapter() {
        let _adapter = OllamaRsAdapter::localhost().unwrap();
    }

    #[test]
    fn llm_provider_completes_through_ollama_client() {
        let (client, server) = fixture_client(
            r#"{"model":"llama3.2","created_at":"now","response":"ok","done":true}"#,
        );
        let provider = OllamaRsLlm::from_client("llama3.2", client).unwrap();

        let response = provider
            .complete(&LlmRequest {
                messages: vec![crate::Message::user("Say ok.")],
                max_tokens: Some(2),
                temperature: Some(0.1),
                ..LlmRequest::default()
            })
            .unwrap();
        let request = server.join().unwrap();

        assert_eq!(response.model, "llama3.2");
        assert_eq!(response.content, "ok");
        assert_eq!(response.stop_reason.as_deref(), Some("done"));
        assert!(request.starts_with("POST /api/generate "));
        assert!(request.contains("\"model\":\"llama3.2\""));
    }

    #[test]
    fn text_embedding_provider_embeds_through_ollama_client() {
        let (client, server) = fixture_client(r#"{"embeddings":[[1.0,2.0,3.0],[4.0,5.0,6.0]]}"#);
        let provider = OllamaRsTextEmbedding::from_client("nomic-embed-text", 3, client).unwrap();

        let embeddings = provider
            .embed(&["alpha".to_string(), "beta".to_string()])
            .unwrap();
        let request = server.join().unwrap();

        assert_eq!(embeddings.len(), 2);
        assert_eq!(embeddings[0], vec![1.0, 2.0, 3.0]);
        assert!(request.starts_with("POST /api/embed "));
        assert!(request.contains("\"model\":\"nomic-embed-text\""));
    }
}
