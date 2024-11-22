use std::{fs, sync::Arc};

use tabby_common::config::ModelConfig;
use tabby_inference::{ChatCompletionStream, CodeGeneration, CompletionStream, Embedding};
use tracing::{info, warn};

#[derive(Clone)]
pub struct PromptInfo {
    pub prompt_template: Option<String>,
    pub chat_template: Option<String>,
}

pub async fn load_embedding(config: &ModelConfig) -> Arc<dyn Embedding> {
    match config {
        ModelConfig::Http(http) => http_api_bindings::create_embedding(http).await,
        ModelConfig::Local(_) => unimplemented!("Local embedding model is not supported"),
    }
}

pub async fn load_code_generation_and_chat(
    completion_model: Option<ModelConfig>,
    chat_model: Option<ModelConfig>,
) -> (
    Option<Arc<CodeGeneration>>,
    Option<Arc<dyn CompletionStream>>,
    Option<Arc<dyn ChatCompletionStream>>,
    Option<PromptInfo>,
) {
    let (engine, prompt_info, chat) =
        load_completion_and_chat(completion_model.clone(), chat_model).await;
    let code = engine
        .clone()
        .map(|engine| Arc::new(CodeGeneration::new(engine, completion_model)));
    (code, engine, chat, prompt_info)
}

async fn load_completion_and_chat(
    completion_model: Option<ModelConfig>,
    chat_model: Option<ModelConfig>,
) -> (
    Option<Arc<dyn CompletionStream>>,
    Option<PromptInfo>,
    Option<Arc<dyn ChatCompletionStream>>,
) {
    let (completion, prompt) = if let Some(completion_model) = completion_model {
        match completion_model {
            ModelConfig::Http(http) => {
                let engine = http_api_bindings::create(&http).await;
                let (prompt_template, chat_template) =
                    http_api_bindings::build_completion_prompt(&http);
                (
                    Some(engine),
                    Some(PromptInfo {
                        prompt_template,
                        chat_template,
                    }),
                )
            }
            ModelConfig::Local(_) => {
                unimplemented!("Local completion model is not supported")
            }
        }
    } else {
        (None, None)
    };

    let chat = if let Some(chat_model) = chat_model {
        match chat_model {
            ModelConfig::Http(http) => Some(http_api_bindings::create_chat(&http).await),
            ModelConfig::Local(_) => {
                unimplemented!("Local chat model is not supported")
            }
        }
    } else {
        None
    };

    (completion, prompt, chat)
}

pub async fn download_model_if_needed(model: &str) {
    if fs::metadata(model).is_ok() {
        info!("Loading model from local path {}", model);
    } else {
        warn!("Model not found at local path: {}", model);
    }
}
