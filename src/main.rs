use anyhow::Result;
use clap::Parser;
use config::Config;
use futures::prelude::*;
use irc::client::prelude as irc_client;
use irc_hook::{message_handler, webhook_publisher};
use std::str::FromStr;
use tracing_subscriber::FmtSubscriber;

/// Joins IRC channels and POSTs webhooks based on regex matching.
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Cli {
    #[arg(short, long, env = "IRC_HOOK_CONFIG_FILE")]
    config_file: String,

    #[arg(short, long, default_value = "warn")]
    log_level: tracing::Level,
}

struct ResolvedConfig {
    nickname: String,
    nick_password: String,
    server: String,
    search_pattern: String,
    webhook_url: http::Uri,
    webhook_api_key: String,
    body_template: String,
}

impl ResolvedConfig {
    pub fn new(settings: Config) -> Result<ResolvedConfig> {
        Ok(ResolvedConfig {
            nickname: settings.get_string("nick")?,
            nick_password: settings.get_string("password")?,
            server: settings.get_string("server")?,
            search_pattern: settings.get_string("search_pattern")?,
            webhook_url: http::Uri::from_str(&settings.get_string("webhook_url")?)?,
            webhook_api_key: settings.get_string("webhook_api_key")?,
            body_template: settings.get_string("body_template")?,
        })
    }
}

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
    let cli = Cli::parse();

    let subscriber = FmtSubscriber::builder()
        .with_env_filter(cli.log_level.as_str())
        .finish();
    tracing::subscriber::set_global_default(subscriber).expect("setting default subscriber failed");

    tracing::info!("starting irc_hook");

    let settings = Config::builder()
        .add_source(config::File::with_name(&cli.config_file))
        .add_source(config::Environment::with_prefix("IRC_HOOK"))
        .build()
        .unwrap();

    let resolved_config = ResolvedConfig::new(settings).unwrap();

    worker(&resolved_config).await
}

async fn worker(conf: &ResolvedConfig) -> Result<()> {
    let irc_config = irc_client::Config {
        nickname: Some(conf.nickname.clone()),
        nick_password: Some(conf.nick_password.clone()),
        server: Some(conf.server.clone()),
        use_tls: Some(true),
        ..irc_client::Config::default()
    };

    let publisher = webhook_publisher::WebhookPublisher::new(
        conf.webhook_url.clone(),
        conf.webhook_api_key.clone(),
        conf.body_template.clone(),
    );

    let mut handler = message_handler::MessageHandler::new(&conf.search_pattern, publisher);

    let mut client = irc_client::Client::from_config(irc_config).await?;
    client.identify()?;

    let mut stream = client.stream()?;

    while let Some(message) = stream.next().await.transpose()? {
        handler.handle_msg(message).await;
    }

    Ok(())
}
