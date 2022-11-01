use futures::prelude::*;
use irc::client::prelude::*;
use irc_hook::{message_handler, webhook_publisher};
use std::{env, str::FromStr};
use tracing_subscriber::FmtSubscriber;

#[tokio::main]
async fn main() -> Result<(), irc::error::Error> {
    let subscriber = FmtSubscriber::builder()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .finish();

    tracing::subscriber::set_global_default(subscriber).expect("setting default subscriber failed");
    tracing::info!("starting irc_hook");

    let resolved_config = ResolvedConfig::build().unwrap();

    let irc_config = Config {
        nickname: Some(resolved_config.nickname),
        nick_password: Some(resolved_config.nick_password),
        server: Some(resolved_config.server),
        use_tls: Some(true),
        ..Config::default()
    };

    let uri = http::Uri::from_str(&env::var("IRC_HOOK_WEBHOOK_URL").unwrap()).unwrap();

    let publisher = webhook_publisher::WebhookPublisher::new(
        uri,
        env::var("IRC_HOOK_WEBHOOK_API_KEY").unwrap(),
        env::var("IRC_HOOK_BODY_TEMPLATE").unwrap(),
    );

    let mut handler = message_handler::MessageHandler::new(
        env::var("IRC_HOOK_SEARCH_PATTERN").unwrap().as_str(),
        publisher,
    );

    let mut client = Client::from_config(irc_config).await?;
    client.identify()?;

    let mut stream = client.stream()?;

    while let Some(message) = stream.next().await.transpose()? {
        handler.handle_msg(message).await;
    }

    Ok(())
}

pub struct ResolvedConfig {
    pub nickname: String,
    pub nick_password: String,
    pub server: String,
}

impl ResolvedConfig {
    pub fn build() -> Result<ResolvedConfig, &'static str> {
        let nickname = env::var("IRC_HOOK_NICK").unwrap();
        let nick_password = env::var("IRC_HOOK_NICK_PASSWORD").unwrap();
        let server = env::var("IRC_HOOK_SERVER").unwrap();

        Ok(ResolvedConfig {
            nickname,
            nick_password,
            server,
        })
    }
}
