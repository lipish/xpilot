use std::{net::IpAddr, sync::Arc, time::Duration};

use axum::{routing, Extension, Router};
use clap::Args;
use hyper::StatusCode;
use spinners::{Spinner, Spinners, Stream};
use tabby_common::{
    api::{self, code::CodeSearch, event::EventLogger},
    axum::AllowedCodeRepository,
    config::{Config, ModelConfig},
    usage,
};
use tabby_inference::CompletionStream;
use tokio::{sync::oneshot::Sender, time::sleep};
use tabby_inference::ChatCompletionStream;
use tower_http::timeout::TimeoutLayer;
use tracing::{debug, warn};
use utoipa::{
    openapi::security::{HttpAuthScheme, HttpBuilder, SecurityScheme},
    Modify, OpenApi,
};
use utoipa_swagger_ui::SwaggerUi;

use crate::{
    routes::{self, run_app},
    services::{
        self,
        code::create_code_search,
        completion::{self, create_completion_service_and_chat, CompletionService},
        embedding,
        event::create_event_logger,
        model::download_model_if_needed,
        tantivy::IndexReaderProvider,
    },
    to_local_config, Device,
};

#[derive(OpenApi)]
#[openapi(
    info(title="Tabby Server",
        description = "
[![tabby stars](https://img.shields.io/github/stars/TabbyML/tabby)](https://github.com/TabbyML/tabby)
[![Join Slack](https://shields.io/badge/Join-Tabby%20Slack-red?logo=slack)](https://links.tabbyml.com/join-slack)

Install following IDE / Editor extensions to get started with [Tabby](https://github.com/TabbyML/tabby).
* [VSCode Extension](https://github.com/TabbyML/tabby/tree/main/clients/vscode) – Install from the [marketplace](https://marketplace.visualstudio.com/items?itemName=TabbyML.vscode-tabby), or [open-vsx.org](https://open-vsx.org/extension/TabbyML/vscode-tabby)
* [VIM Extension](https://github.com/TabbyML/tabby/tree/main/clients/vim)
* [IntelliJ Platform Plugin](https://github.com/TabbyML/tabby/tree/main/clients/intellij) – Install from the [marketplace](https://plugins.jetbrains.com/plugin/22379-tabby)
",
        license(name = "Apache 2.0", url="https://github.com/TabbyML/tabby/blob/main/LICENSE")
    ),
    servers(
        (url = "/", description = "Server"),
    ),
    paths(routes::log_event, routes::completions, routes::setting),
    components(schemas(
        api::event::LogEventRequest,
        completion::CompletionRequest,
        completion::CompletionResponse,
        completion::Segments,
        completion::Declaration,
        completion::Choice,
        completion::Snippet,
        completion::DebugOptions,
        completion::DebugData,
        api::server_setting::ServerSetting,
    )),
    modifiers(&SecurityAddon),
)]
struct ApiDoc;

#[derive(Args)]
pub struct ServeArgs {
    /// Model id for `/completions` API endpoint.
    #[clap(long)]
    model: Option<String>,

    #[clap(long, default_value = "0.0.0.0")]
    host: IpAddr,

    #[clap(long, default_value_t = 8080)]
    port: u16,

    /// Device to run model inference.
    #[clap(long, default_value_t=Device::Cpu)]
    device: Device,

    /// Parallelism for model serving - increasing this number will have a significant impact on the
    /// memory requirement e.g., GPU vRAM.
    #[clap(long, default_value_t = 1)]
    parallelism: u8,
}

pub async fn main(config: &Config, args: &ServeArgs) {
    let config = merge_args(config, args);

    load_model(&config).await;

    let tx = try_run_spinner();

    let embedding = embedding::create(&config.model.embedding).await;

    let mut logger: Arc<dyn EventLogger> = Arc::new(create_event_logger());

    let index_reader_provider = Arc::new(IndexReaderProvider::default());
    let docsearch = Arc::new(services::structured_doc::create(
        embedding.clone(),
        index_reader_provider.clone(),
    ));

    let code = Arc::new(create_code_search(
        embedding.clone(),
        index_reader_provider.clone(),
    ));

    let model = &config.model;
    let (completion_service, _, chat) = create_completion_service_and_chat(
        &config.completion,
        code.clone(),
        logger.clone(),
        model.completion.clone(),
        model.chat.clone(),
    )
    .await;

    let mut api = api_router(
        args,
        &config,
        logger.clone(),
        code.clone(),
        completion_service.map(Arc::new),
        chat,
    )
    .await;
    let mut ui = Router::new()
        .merge(SwaggerUi::new("/swagger-ui").url("/api-docs/openapi.json", ApiDoc::openapi()))
        .fallback(|| async { axum::response::Redirect::temporary("/swagger-ui") });

    if let Some(tx) = tx {
        tx.send(())
            .unwrap_or_else(|_| warn!("Spinner channel is closed"));
    }
    run_app(api, Some(ui), args.host, args.port).await
}

async fn load_model(config: &Config) {
    if let Some(ModelConfig::Local(ref model)) = config.model.completion {
        download_model_if_needed(&model.model_id).await;
    }

    if let ModelConfig::Local(ref model) = config.model.embedding {
        download_model_if_needed(&model.model_id).await;
    }
}

async fn api_router(
    args: &ServeArgs,
    config: &Config,
    logger: Arc<dyn EventLogger>,
    _code: Arc<dyn CodeSearch>,
    completion_state: Option<Arc<CompletionService>>,
    chat_state: Option<Arc<dyn ChatCompletionStream>>,
) -> Router {
    let mut routers = vec![];

    routers.push({
        Router::new()
            .route(
                "/v1/events",
                routing::post(routes::log_event).with_state(logger),
            )
    });

    routers.push({
        Router::new()
            .route(
                "/v1/models",
                routing::get(routes::models).with_state(Arc::new(config.clone().into())),
            )
    });

    if let Some(completion_state) = completion_state {
        let mut router = Router::new()
            .route(
                "/v1/completions",
                routing::post(routes::completions).with_state(completion_state),
            )
            .layer(TimeoutLayer::new(Duration::from_secs(
                config.server.completion_timeout,
            )));

        if let Some(chat_state) = chat_state {
            router = router.route(
                "/v1/chat/completions",
                routing::post(routes::chat_completions).with_state(chat_state),
            );
        }

        router = router.layer(Extension(AllowedCodeRepository::new_from_config()));

        routers.push(router);
    } else {
        routers.push({
            Router::new().route(
                "/v1/completions",
                routing::post(StatusCode::NOT_IMPLEMENTED),
            )
        })
    }

    let server_setting_router =
        Router::new().route("/v1beta/server_setting", routing::get(routes::setting));

    routers.push(server_setting_router);

    let mut root = Router::new();
    for router in routers {
        root = root.merge(router);
    }
    root
}


struct SecurityAddon;

impl Modify for SecurityAddon {
    fn modify(&self, openapi: &mut utoipa::openapi::OpenApi) {
        if let Some(components) = &mut openapi.components {
            components.add_security_scheme(
                "token",
                SecurityScheme::Http(
                    HttpBuilder::new()
                        .scheme(HttpAuthScheme::Bearer)
                        .bearer_format("token")
                        .build(),
                ),
            )
        }
    }
}

fn merge_args(config: &Config, args: &ServeArgs) -> Config {
    let mut config = (*config).clone();
    if let Some(model) = &args.model {
        if config.model.completion.is_some() {
            warn!("Overriding completion model from config.toml. The overriding behavior might surprise you. Consider setting the model in config.toml directly.");
        }
        config.model.completion = Some(to_local_config(model, args.parallelism, &args.device));
    };

    config
}

fn try_run_spinner() -> Option<Sender<()>> {
    if cfg!(feature = "prod") {
        let (tx, rx) = tokio::sync::oneshot::channel();
        tokio::task::spawn(async move {
            let mut sp = Spinner::with_timer_and_stream(
                Spinners::Dots,
                "Starting...".into(),
                Stream::Stdout,
            );
            let _ = rx.await;
            sp.stop_with_message("".into());
        });
        Some(tx)
    } else {
        debug!("Starting server, this might take a few minutes...");
        None
    }
}
