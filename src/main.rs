use anyhow::Result;
use clap::Parser;
use config::Config;
use futures::prelude::*;
use irc::{client::prelude as irc_client, error};
use irc_hook::{message_handler, webhook_publisher};
use std::{collections::HashMap, pin::Pin, str::FromStr};
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
    body_template: String,
    headers: HashMap<&'static str, String>,
}

impl ResolvedConfig {
    pub fn new(settings: Config) -> Result<ResolvedConfig> {
        let headers = settings.get_table("headers").unwrap();

        let headers =
            headers
                .into_iter()
                .fold(HashMap::<&'static str, String>::new(), |mut acc, (k, v)| {
                    acc.insert(Box::leak(k.into_boxed_str()), v.to_string());
                    acc
                });

        Ok(ResolvedConfig {
            nickname: settings.get_string("nick")?,
            nick_password: settings.get_string("password")?,
            server: settings.get_string("server")?,
            search_pattern: settings.get_string("search_pattern")?,
            webhook_url: http::Uri::from_str(&settings.get_string("webhook_url")?)?,
            body_template: settings.get_string("body_template")?,
            headers,
        })
    }
}

struct Worker {
    stream: Pin<Box<dyn Stream<Item = Result<irc::proto::Message, error::Error>>>>,
    handler: message_handler::MessageHandler,
}

impl Worker {
    async fn new(conf: &ResolvedConfig) -> Self {
        let publisher = webhook_publisher::WebhookPublisher::new(
            conf.webhook_url.clone(),
            conf.body_template.clone(),
            conf.headers.clone(),
        );

        let handler = message_handler::MessageHandler::new(&conf.search_pattern, publisher);

        Worker {
            stream: irc_stream(conf).await,
            handler,
        }
    }

    pub async fn run(&mut self) -> Result<(), anyhow::Error> {
        while let Some(message) = self.stream.next().await.transpose()? {
            self.handler.handle_msg(message).await;
        }

        Ok(())
    }
}

async fn irc_stream(
    conf: &ResolvedConfig,
) -> Pin<Box<dyn Stream<Item = Result<irc::proto::Message, error::Error>>>> {
    let irc_config = irc_client::Config {
        nickname: Some(conf.nickname.clone()),
        nick_password: Some(conf.nick_password.clone()),
        server: Some(conf.server.clone()),
        use_tls: Some(true),
        ..irc_client::Config::default()
    };

    let mut client = irc_client::Client::from_config(irc_config).await.unwrap();
    client.identify().unwrap();

    Box::pin(client.stream().unwrap())
}

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
    let cli = Cli::parse();

    let subscriber = FmtSubscriber::builder()
        .with_env_filter(cli.log_level.as_str())
        .finish();
    tracing::subscriber::set_global_default(subscriber).expect("setting default subscriber failed");

    let settings = Config::builder()
        .add_source(config::File::with_name(&cli.config_file))
        .add_source(config::Environment::with_prefix("IRC_HOOK"))
        .build()
        .unwrap();

    let conf = ResolvedConfig::new(settings).unwrap();

    tracing::info!("starting irc_hook");

    let mut worker = Worker::new(&conf).await;
    worker.run().await
}

// #[cfg(test)]
// mod tests {
//     use super::*;
//     use httptest::{matchers::*, responders::*, Expectation, Server};

//     #[tokio::test]
//     async fn test_app() {
//         let cli = Cli {
//             config_file: "".to_string(),
//             log_level: tracing::Level::DEBUG,
//         };

//         let server = Server::run();
//         server.expect(
//             Expectation::matching(request::method_path("POST", "/endpoint"))
//                 .times(2)
//                 .respond_with(status_code(200)),
//         );
//         let uri = server.url("/endpoint");

//         let settings = Config::builder()
//             .add_source(config::File::with_name(&cli.config_file))
//             .add_source(config::Environment::with_prefix("IRC_HOOK"))
//             .build()
//             .unwrap();

//         let conf = ResolvedConfig::new(settings).unwrap();

//         let mut worker = Worker::new(&conf).await;

//         worker.stream = irc_stream(&conf).await;

//         worker.run().await.unwrap();

// let content = r#"Main message 1capture match2 text 1another match2"#;
// let search_pattern = r#"\d(.+?)\d"#;

// let server = Server::run();
// server.expect(
//     Expectation::matching(request::method_path("POST", "/endpoint"))
//         .times(2)
//         .respond_with(status_code(200)),
// );
// let uri = server.url("/endpoint");

// let publisher = webhook_publisher::WebhookPublisher::new(
//     uri,
//     "".to_string(),
//     std::collections::HashMap::new(),
// );
// let mut handler = MessageHandler::new(search_pattern, publisher);

// let msg = Message::new(Some("user"), "PRIVMSG", vec!["#channel", content]).unwrap();

// handler.handle_msg(msg).await;
//     }
// }
