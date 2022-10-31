use futures::prelude::*;
use irc::client::prelude::*;
use regex::Regex;
use std::{env, str::FromStr, sync::Arc};
use tokio::task;
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

    let publisher = WebhookPublisher::new(
        uri,
        env::var("IRC_HOOK_WEBHOOK_API_KEY").unwrap(),
        env::var("IRC_HOOK_BODY_TEMPLATE").unwrap(),
    );

    let mut handler = MessageHandler::new(env::var("IRC_HOOK_SEARCH_PATTERN").unwrap(), publisher);

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

struct MessageHandler {
    search_pattern: String,
    message_publisher: WebhookPublisher,
}

impl MessageHandler {
    fn new(search_pattern: String, message_publisher: WebhookPublisher) -> Self {
        MessageHandler {
            search_pattern,
            message_publisher,
        }
    }

    async fn handle_msg(&mut self, msg: Message) {
        if let Some(content) = get_content(&msg.to_string()) {
            let re = Regex::new(&self.search_pattern).unwrap();

            tracing::debug!(msg = content, "checking for matches");
            if !re.is_match(&content) {
                return;
            }
            tracing::info!(%content, "matched");

            let groups = match_groups(&re, &content);
            self.message_publisher.publish(groups).await;
        }
    }
}

fn match_groups(re: &regex::Regex, content: &str) -> Vec<Vec<String>> {
    re.captures_iter(content)
        .map(|group| {
            group
                .iter()
                .filter_map(|mat| Some(mat?.as_str().to_string()))
                .collect()
        })
        .collect()
}

#[derive(Debug)]
struct WebhookPublisher {
    endpoint: http::Uri,
    api_key: String,
    template: String,
}

impl WebhookPublisher {
    fn new(endpoint: http::Uri, api_key: String, template: String) -> Self {
        WebhookPublisher {
            endpoint,
            api_key,
            template,
        }
    }

    async fn publish(&self, params: Vec<Vec<String>>) {
        let ep = Arc::new(self.endpoint.to_string());
        let key = Arc::new(self.api_key.clone());
        let template = Arc::new(self.template.clone());

        let tasks = params
            .into_iter()
            .map(|p_set| {
                task::spawn({
                    let ep_clone = Arc::clone(&ep);
                    let key_clone = Arc::clone(&key);
                    let template_clone = Arc::clone(&template);

                    async move {
                        tracing::debug!(params = ?p_set, "building POST body");

                        // Replace the items in the template with matches from the group.
                        let body_init = template_clone.to_string();
                        let body = p_set
                            .iter()
                            .enumerate()
                            .fold(body_init, |body, (idx, repl)| {
                                let idx_thing = format!("${{{}}}", idx);
                                body.replace(&idx_thing, repl)
                            });
                        tracing::debug!(body = body);

                        let client = reqwest::Client::new();
                        let res = client
                            .post(ep_clone.as_str())
                            .header("X-Api-Key", key_clone.to_string())
                            .header("Content-Type", "application/json")
                            .body(body)
                            .send()
                            .await;

                        println!("{:#?}", res);
                    }
                })
            })
            .collect::<stream::FuturesUnordered<_>>();

        let result = futures::future::join_all(tasks).await;
        println!("{:?}", result);
    }
}

fn get_content(m: &str) -> Option<String> {
    // Skip the first character, which is always a :, then find the next :
    let mut chrs = m.chars().skip(1);

    if let Some(_) = chrs.find(|&c| c == ':') {
        // Make sure the trailing newline is removed.
        Some(chrs.collect::<String>().trim().to_string())
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use httptest::{matchers::*, responders::*, Expectation, Server};

    #[test]
    fn test_get_content() {
        let input = ":first part:Hello this is a message".to_string();
        assert_eq!(
            get_content(&input),
            Some("Hello this is a message".to_string())
        );
    }

    #[test]
    fn test_match_groups() {
        let content = r#"Main message 1capture match2 text 1another match2"#;

        let search_pattern = r#"\d(.+?)\d"#;
        let re = Regex::new(&search_pattern).unwrap();

        let got = match_groups(&re, content);

        assert_eq!(
            got,
            vec![
                vec!["1capture match2".to_string(), "capture match".to_string()],
                vec!["1another match2".to_string(), "another match".to_string()]
            ]
        )
    }

    #[tokio::test]
    async fn test_handler_() {
        let content = r#"Main message 1capture match2 text 1another match2"#;
        let search_pattern = r#"\d(.+?)\d"#;

        let server = Server::run();
        server.expect(
            Expectation::matching(request::method_path("POST", "/endpoint"))
                .times(2)
                .respond_with(status_code(200)),
        );
        let uri = server.url("/endpoint");

        let publisher = WebhookPublisher::new(uri, "".to_string(), "".to_string());
        let mut handler = MessageHandler::new(search_pattern.to_string(), publisher);

        let msg = Message::new(Some("user"), "PRIVMSG", vec!["#channel", content]).unwrap();

        handler.handle_msg(msg).await;
    }
}
